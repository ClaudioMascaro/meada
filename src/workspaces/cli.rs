//! `meada workspaces` (#343, ADR 0021 contract 4): the command-line twins of the console's
//! workspace operations, acting as the platform administrator with `cli:<login>@<host>` as the
//! actor, as `meada workspace` does. The CLI holds the bootstrap manifest's Postgres and store
//! credentials, so it is the global operator, and it asks the same atomic boundary
//! (`lifecycle::transact`, `store::create`) the console does: a stale base, a missing catalog
//! entry or a refusal changes nothing. Every write is an audit record (`audit::cli`), inserted
//! before it runs; reads are not audited. The answer is on stdout, the reason of a refusal on
//! stderr with exit 1.

use std::path::Path;

use meada_core::authorize::Grant;
use meada_core::named;
use serde_json::{Value as Json, json};

use super::lifecycle::{self as ops, Error, Op, Target};
use super::store::{self, CreateError, Creator, New, Policy, Record};
use crate::msg::Msg;
use crate::workspace::Manifest;

const ADMINISTRATOR: &str = "platform_administrator";

/// The sections a command reads, from the bootstrap manifest.
struct Env {
    manifest: Manifest,
    author: String,
}

impl Env {
    fn read(file: &Path) -> crate::Result<Self> {
        let manifest = Manifest::read(file)?;
        let missing = || Msg::KeyMissing("workspaces").text();
        let ready = manifest.workspaces.is_some()
            && manifest.postgres.is_some()
            && manifest.snapshot.is_some()
            && (manifest.oidc.as_ref()).is_some_and(|o| o.github.is_some());
        if !ready {
            return Err(missing().into());
        }
        Ok(Self {
            manifest,
            author: crate::snapshot::versions::cli_author(),
        })
    }

    fn postgres(&self) -> &crate::postgres::Config {
        self.manifest.postgres.as_ref().expect("checked by read")
    }

    fn snapshot(&self) -> &crate::snapshot::Config {
        self.manifest.snapshot.as_ref().expect("checked by read")
    }

    fn section(&self) -> &super::Config {
        self.manifest.workspaces.as_ref().expect("checked by read")
    }

    fn policy(&self) -> Policy<'_> {
        let github = (self.manifest.oidc.as_ref()).and_then(|o| o.github.as_ref());
        let github = github.expect("checked by read");
        Policy {
            postgres: self.postgres(),
            max_age: github.max_age,
            roles: &github.roles,
            max_workspaces: self.section().max_workspaces,
        }
    }

    fn actor(&self) -> Creator<'_> {
        Creator {
            subject: &self.author,
            login: None,
            administrator: true,
        }
    }

    fn record(&self, id: &str) -> Result<Record, OpError> {
        let max = self.section().max_workspaces;
        let found = crate::modules::block_on(store::records(self.postgres(), Some(id), max));
        let found = found.map_err(|e| e.to_string()).and_then(|r| r);
        match found {
            Ok(mut records) => {
                (records.pop()).ok_or_else(|| OpError::refused(Msg::WorkspacesNotFound))
            }
            Err(why) => Err(OpError::failed(&why)),
        }
    }
}

/// Why a command did nothing: its message, and whether it was the operation's own refusal.
#[derive(Debug)]
pub struct OpError {
    text: String,
    refused: bool,
}

impl OpError {
    fn refused(message: Msg<'_>) -> Self {
        Self {
            text: message.text(),
            refused: true,
        }
    }

    fn failed(why: &str) -> Self {
        Self {
            text: Msg::PostgresUnreachable(why).text(),
            refused: false,
        }
    }

    fn of(error: Error) -> Self {
        match error {
            Error::NotFound => Self::refused(Msg::WorkspacesNotFound),
            Error::Stale(seq) => Self::refused(Msg::WorkspacesStale(seq)),
            Error::NotOperator => Self::refused(Msg::WorkspacesNotOperator),
            Error::NotAdministrator => Self::refused(Msg::WorkspacesNotAdministrator),
            Error::Pending(reason) => Self::refused(Msg::WorkspacesPending(reason)),
            Error::Refused(decided) => {
                let count = decided.named.len() + decided.hidden;
                let names = decided.named.join(", ");
                Self {
                    text: format!("{} ({names})", Msg::WorkspacesRefused(count).text()),
                    refused: true,
                }
            }
            Error::UnknownVersion => Self::refused(Msg::WorkspacesUnknownVersion),
            Error::AlreadyCurrent => Self::refused(Msg::WorkspacesAlreadyCurrent),
            Error::Operators(reason) => Self::refused(Msg::WorkspacesOperators(reason)),
            Error::Failed(why) => Self::failed(&why),
            Error::Db(_) => Self::failed("database"),
        }
    }
}

impl std::fmt::Display for OpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.text)
    }
}

impl std::error::Error for OpError {}

/// A manifest file read within the named text's bound, as the entries of a version.
fn entries_of(env: &Env, file: &Path) -> crate::Result<Vec<Json>> {
    let bytes = crate::config::read_bounded(file, named::TEXT_MAX as u64 + 1)?;
    let text = String::from_utf8(bytes).map_err(|_| Msg::WorkspacesBody.text())?;
    let shown = file.display().to_string();
    let entries = named::version_entries(&shown, &text, env.section().max_entries)
        .map_err(|problems| problems.join("\n"))?;
    match entries.is_empty() {
        true => Err(Msg::UiEntriesInvalid.text().into()),
        false => Ok(entries),
    }
}

/// The audit of a write by `env`'s author: `action` with `arguments`, `op` run once the record
/// is inserted.
fn audited<T>(
    env: &Env,
    manifest: &Path,
    action: &str,
    arguments: Json,
    op: impl FnOnce() -> Result<T, OpError>,
    accepted: impl FnOnce(&T) -> String,
) -> crate::Result<T> {
    crate::audit::cli(
        manifest,
        &env.author,
        action,
        Some(arguments),
        op,
        accepted,
        |e: &OpError| e.refused,
    )
}

fn arguments(id: &str, base: i64, version: Option<&str>) -> Json {
    let mut arguments = json!({
        "workspace": id, "base": base.to_string(), "authority": ADMINISTRATOR,
    });
    if let Some(version) = version {
        arguments["version"] = version.into();
    }
    arguments
}

/// `list`: one line per workspace, `<id>\t<version>\t<base>\t<name>`.
pub fn list(manifest: &Path) -> crate::Result<String> {
    let env = Env::read(manifest)?;
    let max = env.section().max_workspaces;
    let records = crate::modules::block_on(store::records(env.postgres(), None, max))?;
    let records = records?;
    Ok(records
        .iter()
        .map(|r| format!("{}\t{}\t{}\t{}\n", r.id, r.current, r.seq, r.name))
        .collect())
}

/// `show`: the workspace's header as comments and the entries of its current version, or of
/// `version`, as `[[repository]]` entries.
pub fn show(manifest: &Path, id: &str, version: Option<&str>) -> crate::Result<String> {
    let env = Env::read(manifest)?;
    let record = env.record(id)?;
    let shown = version.unwrap_or(&record.current);
    if !meada_core::ids::is_id(shown) {
        return Err(Msg::WorkspacesUnknownVersion.text().into());
    }
    if version.is_some() {
        let row = crate::modules::block_on(ops::version(env.postgres(), id, shown))?;
        if row?.is_none() {
            return Err(Msg::WorkspacesUnknownVersion.text().into());
        }
    }
    let entries = store::get_document(env.snapshot(), shown)?;
    let operators: Vec<String> = (record.operators.iter())
        .map(|(subject, login)| format!("{subject} ({})", login.as_deref().unwrap_or("-")))
        .collect();
    let mut text = format!(
        "# workspace {id}\n# name: {}\n# base: {}\n# version: {shown}\n# operators: {}\n\n",
        record.name,
        record.seq,
        operators.join(", ")
    );
    text += &meada_core::workspace::entries_text(&entries).unwrap_or_default();
    Ok(text)
}

/// `create`: the workspace `name` of the entries of `file`; its id.
pub fn create(
    manifest: &Path,
    file: &Path,
    name: &str,
    description: &str,
    message: &str,
) -> crate::Result<String> {
    let env = Env::read(manifest)?;
    if let Some(problem) = named::name_problem(name)
        .or_else(|| named::description_problem(description))
        .or_else(|| named::description_problem(message))
        .or_else(|| message.is_empty().then(|| Msg::WorkspacesBody.text()))
    {
        return Err(problem.into());
    }
    let entries = entries_of(&env, file)?;
    let version = named::version_id(&entries);
    let id = meada_core::ids::workspace_id(&env.author, name, &version);
    let line = audited(
        &env,
        manifest,
        "workspaces.create",
        json!({"workspace": id, "version": version, "authority": ADMINISTRATOR}),
        || {
            store::put_document(env.snapshot(), &version, &entries)
                .map_err(|e| OpError::failed(&e))?;
            let new = New {
                name,
                description,
                version: &version,
                entries: &entries,
                message,
            };
            let made = crate::modules::block_on(store::create(
                &env.policy(),
                &env.actor(),
                &Grant::All,
                &new,
            ))
            .map_err(|e| OpError::failed(&e.to_string()))?;
            match made {
                Ok(_) => Ok(id.clone()),
                Err(CreateError::Pending(reason)) => {
                    Err(OpError::refused(Msg::WorkspacesPending(reason)))
                }
                Err(CreateError::Refused(decided)) => {
                    let count = decided.named.len() + decided.hidden;
                    Err(OpError::refused(Msg::WorkspacesRefused(count)))
                }
                Err(CreateError::Limit) => {
                    let max = env.section().max_workspaces;
                    Err(OpError::refused(Msg::WorkspacesLimit(max)))
                }
                Err(CreateError::Deleted) => Err(OpError::refused(Msg::WorkspaceDeleted)),
                Err(CreateError::Failed(why)) => Err(OpError::failed(&why)),
            }
        },
        |id| id.clone(),
    )?;
    Ok(format!("{line}\n"))
}

/// What a write on an existing workspace resolved before its transaction.
struct Resolved {
    base: i64,
    current: String,
}

fn resolve(env: &Env, id: &str, base: i64) -> Result<Resolved, OpError> {
    let record = env.record(id)?;
    match record.seq == base {
        true => Ok(Resolved {
            base,
            current: record.current,
        }),
        false => Err(OpError::refused(Msg::WorkspacesStale(record.seq))),
    }
}

/// Runs `op` on workspace `id` at `base`; the new base.
fn transact(env: &Env, id: &str, resolved: &Resolved, op: &Op<'_>) -> Result<ops::Done, OpError> {
    let target = Target {
        id,
        base: resolved.base,
        current: &resolved.current,
    };
    let done = crate::modules::block_on(ops::transact(
        &env.policy(),
        &env.actor(),
        &Grant::All,
        &target,
        op,
    ));
    done.map_err(|e| OpError::failed(&e.to_string()))?
        .map_err(OpError::of)
}

/// `save`/`import`: stores the entries of `file` as a version of `id` and moves nothing; the
/// version id.
pub fn save(
    manifest: &Path,
    id: &str,
    file: &Path,
    message: &str,
    base: i64,
) -> crate::Result<String> {
    let env = Env::read(manifest)?;
    if let Some(problem) = named::description_problem(message)
        .or_else(|| message.is_empty().then(|| Msg::WorkspacesBody.text()))
    {
        return Err(problem.into());
    }
    let submitted = entries_of(&env, file)?;
    let version = named::version_id(&submitted);
    let line = audited(
        &env,
        manifest,
        "workspaces.save",
        arguments(id, base, Some(&version)),
        || {
            let resolved = resolve(&env, id, base)?;
            let current = store::get_document(env.snapshot(), &resolved.current)
                .map_err(|e| OpError::failed(&e))?;
            let changed = named::changed(&current, &submitted);
            let identities = named::identities(&submitted);
            store::put_document(env.snapshot(), &version, &submitted)
                .map_err(|e| OpError::failed(&e))?;
            let op = Op::Save {
                version: &version,
                message,
                identities: &identities,
                changed: &changed,
            };
            transact(&env, id, &resolved, &op)?;
            Ok(version.clone())
        },
        |version| version.clone(),
    )?;
    Ok(format!("{line}\n"))
}

/// `apply`: makes `version` the current one of `id`; `apply` or `rollback`, the version and the
/// new base.
pub fn apply(manifest: &Path, id: &str, version: &str, base: i64) -> crate::Result<String> {
    let env = Env::read(manifest)?;
    if !meada_core::ids::is_id(version) {
        return Err(Msg::WorkspacesUnknownVersion.text().into());
    }
    let was = crate::modules::block_on(ops::was_current(env.postgres(), id, version))?;
    let action = match was? {
        true => "workspaces.rollback",
        false => "workspaces.apply",
    };
    let line = audited(
        &env,
        manifest,
        action,
        arguments(id, base, Some(version)),
        || {
            let resolved = resolve(&env, id, base)?;
            let known = crate::modules::block_on(ops::version(env.postgres(), id, version));
            let known = known.map_err(|e| e.to_string()).and_then(|r| r);
            if known.map_err(|e| OpError::failed(&e))?.is_none() {
                return Err(OpError::refused(Msg::WorkspacesUnknownVersion));
            }
            let current = store::get_document(env.snapshot(), &resolved.current)
                .map_err(|e| OpError::failed(&e))?;
            let target =
                store::get_document(env.snapshot(), version).map_err(|e| OpError::failed(&e))?;
            let changed = named::changed(&current, &target);
            let identities = named::identities(&target);
            let op = Op::Apply {
                version,
                identities: &identities,
                changed: &changed,
            };
            let done = transact(&env, id, &resolved, &op)?;
            Ok(format!("{} {version} base {}", done.action, done.seq))
        },
        |line| line.clone(),
    )?;
    Ok(format!("{line}\n"))
}

/// `rename`: the new name of `id`, and its description when one is given; the new base.
pub fn rename(
    manifest: &Path,
    id: &str,
    name: &str,
    description: Option<&str>,
    base: i64,
) -> crate::Result<String> {
    let env = Env::read(manifest)?;
    let line = audited(
        &env,
        manifest,
        "workspaces.rename",
        arguments(id, base, None),
        || {
            let record = env.record(id)?;
            let description = description.unwrap_or(&record.description);
            if let Some(problem) =
                named::name_problem(name).or_else(|| named::description_problem(description))
            {
                return Err(OpError {
                    text: problem,
                    refused: true,
                });
            }
            let resolved = resolve(&env, id, base)?;
            let op = Op::Rename { name, description };
            let done = transact(&env, id, &resolved, &op)?;
            Ok(format!("base {}", done.seq))
        },
        |line| line.clone(),
    )?;
    Ok(format!("{line}\n"))
}

/// `delete`: deletes `id`, keeping its versions and history; the new base.
pub fn delete(manifest: &Path, id: &str, base: i64) -> crate::Result<String> {
    let env = Env::read(manifest)?;
    let line = audited(
        &env,
        manifest,
        "workspaces.delete",
        arguments(id, base, None),
        || {
            let resolved = resolve(&env, id, base)?;
            let done = transact(&env, id, &resolved, &Op::Delete)?;
            Ok(format!("deleted base {}", done.seq))
        },
        |line| line.clone(),
    )?;
    Ok(format!("{line}\n"))
}

/// `operators`: adds `add` (`<subject>=<login>`) and removes `remove` (subjects) from the
/// operators of `id`; the new base.
pub fn operators(
    manifest: &Path,
    id: &str,
    add: &[String],
    remove: &[String],
    base: i64,
) -> crate::Result<String> {
    let env = Env::read(manifest)?;
    let invalid = || OpError::refused(Msg::WorkspacesOperators("invalid"));
    let mut added = Vec::new();
    for pair in add {
        let (subject, login) = pair.split_once('=').ok_or_else(invalid)?;
        added.push((subject.to_string(), login.to_string()));
    }
    if !ops::operators_valid(&added, remove) {
        return Err(invalid().into());
    }
    let line = audited(
        &env,
        manifest,
        "workspaces.operators",
        arguments(id, base, None),
        || {
            let resolved = resolve(&env, id, base)?;
            let op = Op::Operators {
                add: &added,
                remove,
            };
            let done = transact(&env, id, &resolved, &op)?;
            Ok(format!("base {}", done.seq))
        },
        |line| line.clone(),
    )?;
    Ok(format!("{line}\n"))
}

/// `versions`: one line per version of `id`, newest first, `<version>\t<saved>\t<current|->\t
/// <author>\t<message>`, the current one marked.
pub fn versions(manifest: &Path, id: &str) -> crate::Result<String> {
    let env = Env::read(manifest)?;
    env.record(id)?;
    let mut text = String::new();
    let mut after: Option<String> = None;
    loop {
        let page = crate::modules::block_on(ops::versions(
            env.postgres(),
            id,
            None,
            after.as_deref(),
            ops::PAGE_MAX + 1,
        ))?;
        let mut page = page?;
        let more = page.len() > ops::PAGE_MAX;
        page.truncate(ops::PAGE_MAX);
        for row in &page {
            let mark = if row.current { "current" } else { "-" };
            text += &format!(
                "{}\t{}\t{mark}\t{}\t{}\n",
                row.version, row.saved, row.author, row.message
            );
        }
        after = page.last().map(|row| row.cursor.clone());
        if !more || after.is_none() {
            return Ok(text);
        }
    }
}

/// `diff`: what `version` of `id` adds, drops and changes against `against` (a version id or
/// `current`), as lines `added <repo>`, `removed <repo>` and `changed <repo> <keys>`.
pub fn diff(manifest: &Path, id: &str, version: &str, against: &str) -> crate::Result<String> {
    let env = Env::read(manifest)?;
    let record = env.record(id)?;
    let against = match against {
        "current" => record.current.as_str(),
        other => other,
    };
    let mut documents = Vec::new();
    for shown in [against, version] {
        let row = match meada_core::ids::is_id(shown) {
            true => crate::modules::block_on(ops::version(env.postgres(), id, shown))??,
            false => None,
        };
        if row.is_none() {
            return Err(Msg::WorkspacesUnknownVersion.text().into());
        }
        documents.push(store::get_document(env.snapshot(), shown)?);
    }
    let diff = named::diff(&documents[0], &documents[1]);
    let mut text = String::new();
    for repo in &diff.added {
        text += &format!("added {repo}\n");
    }
    for repo in &diff.removed {
        text += &format!("removed {repo}\n");
    }
    for change in &diff.changed {
        text += &format!("changed {} {}\n", change.repo, change.keys.join(","));
    }
    Ok(text)
}
