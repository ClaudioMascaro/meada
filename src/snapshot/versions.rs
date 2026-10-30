//! Workspace versions (ADR 0014, feature `generation-pointer`): the `[[repository]]` entries of a
//! workspace as immutable documents in the snapshot store, and `meada workspace import`, `list`
//! and `show`. The console calls the same functions.
//!
//! A version is stored at `workspaces/<version id>.json` under the store's prefix, with its size
//! and BLAKE3 hash as metadata, and never rewritten: `id`, `author`, `time` (RFC 3339, UTC),
//! `message` and `repositories`, in manifest order, each entry its `repo` and `path` as written,
//! its `owner` when it has one (ADR 0015) and its blocks as `entry_blocks` gives them. The id is
//! `meada_core`'s workspace version id of the entries alone. The current version is `{<key_prefix>}:workspace`, a hash `{id, fence}`
//! under the pointer's hash tag, which `meada workspace apply` moves. Everything read back from
//! the store or Valkey is hostile: each name, id and fence is checked against its pattern, and a
//! document is bounded in size, its entries validated again and its id derived from them again
//! before it is used.
//!
//! `meada workspace pull` brings the current version onto the machine, under
//! `<index_dir>/workspace/` (`DIR`): `<id>.json`, the document as stored, and `current`, one line
//! `<id> <fence>`, swapped by rename and never to a lower fence; `pull.lock` keeps one pull at a
//! time. `index`, `status`, `export` and `snapshot publish` read the version `current` names
//! from there (`pulled`, through `Manifest::read_listed`), checked again, and never connect.
//! Under `[oidc.github]` they read it without the additions `meada github-app roles` excluded
//! (`eligible`), which it writes to `excluded` (`EXCLUDED`) beside `current` (#331).

#[cfg(feature = "refresh-lock")]
use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::fs::{self, File};
use std::path::{Path, PathBuf};

#[cfg(feature = "refresh-lock")]
use meada_core::authorize::{self, Grant, Refusal, Role, authorize_entries};
#[cfg(all(feature = "ui", feature = "refresh-lock"))]
use meada_core::authorize::{Change, Reason, compose_entries};
use meada_core::ids::{VersionEntry, workspace_version_id};
use meada_core::manifest::{EntryFile, validate, validate_import};
use meada_core::workspace::{entries_text, entry};
use redis::cmd;
use serde::{Deserialize, Serialize};
use serde_json::Value as Json;

#[cfg(feature = "refresh-lock")]
use super::generations::OpError;
use super::generations::{now, rfc3339};
use super::store::Store;
use super::{Config, entry_blocks, fence, is_id, pull};
use crate::msg::Msg;
use crate::valkey::{self, Valkey};
use crate::workspace::Manifest;

// ponytail: the most a workspace version document may hold in the store; past it the document
// is refused, naming its key.
pub(crate) const VERSION_MAX: u64 = 4 << 20;

/// The directory under `index_dir` where `pull` leaves the current version.
pub const DIR: &str = "workspace";
/// The current file in `DIR`: `<id> <fence>`.
const CURRENT: &str = "current";

/// A workspace version document (`workspaces/<id>.json`), never rewritten once stored.
#[derive(Serialize, Deserialize)]
pub struct Version {
    pub id: String,
    pub author: String,
    /// RFC 3339, UTC, to the second.
    pub time: String,
    pub message: String,
    /// In manifest order: each entry's `repo` and `path` as written, its `owner` if any, and its
    /// blocks.
    pub repositories: Vec<Json>,
}

/// The sections the workspace commands need from the bootstrap manifest.
struct Sections {
    snapshot: Config,
    valkey: valkey::Config,
    index_dir: PathBuf,
    /// With `[workspaces]`, where the named workspaces are read from (#342).
    #[cfg(feature = "workspaces")]
    named: Option<(crate::postgres::Config, crate::workspaces::Config)>,
}

/// `manifest`'s `[snapshot]` and `[valkey]`, or the error naming it when it has no
/// `[generations]` (which needs both, `Manifest::read`).
fn sections(manifest: &Path) -> crate::Result<Sections> {
    let read = Manifest::read(manifest)?;
    match (read.generations, read.snapshot, read.valkey) {
        (Some(_), Some(snapshot), Some(valkey)) => Ok(Sections {
            snapshot,
            valkey,
            index_dir: read.index_dir,
            #[cfg(feature = "workspaces")]
            named: read.postgres.zip(read.workspaces),
        }),
        _ => Err(Msg::WorkspaceNoSections(&manifest.display().to_string())
            .text()
            .into()),
    }
}

/// The current version's pointer, `{<key_prefix>}:workspace`.
pub fn pointer_key(valkey: &valkey::Config) -> String {
    valkey.key("workspace", true)
}

/// The key of the version `id` in the store.
pub(crate) fn version_key(snapshot: &Config, id: &str) -> String {
    snapshot.key(&format!("workspaces/{id}.json"))
}

/// The current version's id and fence, checked; `None` when no version is current. A malformed
/// pointer is an error naming its key.
pub async fn current(
    valkey: &Valkey,
    config: &valkey::Config,
) -> Result<Option<(String, u64)>, String> {
    let key = pointer_key(config);
    let fields = cmd("HMGET").arg(&key).arg("id").arg("fence").clone();
    let fields: Vec<Option<Vec<u8>>> =
        (valkey.query(&fields).await).map_err(|e| format!("{key}: {e}"))?;
    let text = |bytes: &[u8]| std::str::from_utf8(bytes).ok().map(str::to_string);
    match fields.as_slice() {
        [None, None] => Ok(None),
        [Some(id), Some(f)] => match (
            text(id).filter(|id| is_id(id)),
            text(f).and_then(|f| fence(&f)),
        ) {
            (Some(id), Some(fence)) => Ok(Some((id, fence))),
            _ => Err(Msg::WorkspaceBadPointer(&key).text()),
        },
        _ => Err(Msg::WorkspaceBadPointer(&key).text()),
    }
}

/// What `import` did: the version's id and, when its entries were stored already, the version
/// as first stored (nothing was uploaded).
pub struct Imported {
    pub id: String,
    pub first: Option<Version>,
}

/// `meada workspace import <file> --message <text> --workspace <manifest>`: the `[[repository]]`
/// entries of `file` stored as a version by `author`. The file's other top-level keys go to
/// `notes`, each named as not imported. Every check the text decides runs before any connection
/// opens; an error lists each problem on its own line. The version is not applied.
pub fn import(
    manifest: &Path,
    file: &Path,
    message: &str,
    author: &str,
    notes: &mut Vec<String>,
) -> crate::Result<Imported> {
    let sections = sections(manifest)?;
    let shown = file.display().to_string();
    let src = std::fs::read_to_string(file).map_err(|e| format!("{shown}: {e}"))?;
    if let Ok(table) = toml::from_str::<toml::Table>(&src) {
        let others = table.keys().filter(|k| *k != "repository");
        let not_imported = Msg::WorkspaceNotImported.text();
        notes.extend(
            others.map(|k| crate::config::error_at(file, &src, k, &not_imported).to_string()),
        );
    }
    let version =
        prepared(&shown, &src, message, author).map_err(|problems| problems.join("\n"))?;
    let run = async {
        let store = Store::connect(&sections.snapshot)?;
        store_version(&store, &sections.snapshot, &version).await
    };
    Ok(crate::modules::block_on(run)??)
}

/// The same text preparation for CLI import and HTTP save, before either opens a connection.
fn prepared(file: &str, src: &str, message: &str, author: &str) -> Result<Version, Vec<String>> {
    let entries = validate_import(file, src)?;
    let (id, repositories) = versioned(&entries, entry_blocks(src));
    Ok(Version {
        id,
        author: author.to_string(),
        time: rfc3339(now()),
        message: message.to_string(),
        repositories,
    })
}

async fn store_version(
    store: &Store,
    snapshot: &Config,
    version: &Version,
) -> Result<Imported, String> {
    let id = version.id.clone();
    let key = version_key(snapshot, &id);
    if let Some((first, _)) = stored(store, &key, &id).await? {
        return Ok(Imported {
            id,
            first: Some(first),
        });
    }
    let mut text = serde_json::to_vec_pretty(version).map_err(|e| e.to_string())?;
    text.push(b'\n');
    if text.len() as u64 > VERSION_MAX {
        return Err(Msg::WorkspaceVersionTooLarge {
            key: &key,
            max: VERSION_MAX,
        }
        .text());
    }
    let first = if store.create(&key, text).await? {
        None
    } else {
        Some(
            stored(store, &key, &id)
                .await?
                .ok_or_else(|| Msg::WorkspaceNotVersion(&key).text())?
                .0,
        )
    };
    Ok(Imported { id, first })
}

/// A text validation refusal is different from a lock/store refusal at the HTTP boundary.
#[cfg(all(feature = "ui", feature = "refresh-lock"))]
pub(crate) enum SaveError {
    Invalid(Vec<String>),
    /// An operation's own refusal: the lock held or lost.
    Refused(String),
    /// Anything else: the manifest, the store or Valkey.
    Failed(String),
    /// The entries a team operator may not change, as `authorize_entries` lists them.
    Unauthorized(Vec<Refusal>),
    /// A team operator that the current version no longer makes one.
    RoleLost,
}

/// Who a write acts as: its role, its groups and grant, and under the GitHub grant provider
/// (ADR 0016) its login and the GitHub grant provider's host, both none without it.
#[cfg(feature = "refresh-lock")]
pub struct Acting<'a> {
    pub role: Role,
    pub groups: &'a [String],
    pub grant: &'a Grant,
    pub login: Option<&'a str>,
    pub github_host: Option<&'a str>,
}

#[cfg(feature = "refresh-lock")]
impl Acting<'_> {
    /// The global operator, as the CLI acts.
    pub fn operator() -> Acting<'static> {
        Acting {
            role: Role::Operator,
            groups: &[],
            grant: &Grant::All,
            login: None,
            github_host: None,
        }
    }

    /// `authorize_entries` of the change from `current` to `new`.
    fn refused(&self, current: &[Json], new: &[Json]) -> Vec<Refusal> {
        let Acting {
            role,
            groups,
            grant,
            login,
            github_host,
        } = *self;
        authorize_entries(role, groups, grant, login, github_host, current, new)
    }
}

/// The console's save is CLI import's text preparation and storage under the refresh lock.
/// CLI import deliberately remains available with generation-pointer alone and takes no lock.
/// Anyone but the global operator (`acting`) is authorized under the lock against the version
/// then current, which two writes cannot move at once: its role recomputed, the submitted
/// entries composed over the current entries its grant reads (`compose_entries`, which under the
/// GitHub grant provider attributes its additions, so a forged `added_by` decides nothing),
/// `authorize_entries` over that change, an addition of an identity the version holds outside
/// the grant refused `outside_grant` (`collisions`), then the version stored is
/// `compose_entries`' over the whole current version (ADR 0015, #333). The global operator's
/// text is stored as written, `added_by` included. Returns what was stored with the owner groups
/// of the entries the change touches, for a team operator's audit line.
#[cfg(all(feature = "ui", feature = "refresh-lock"))]
pub(crate) fn save(
    manifest: &Path,
    src: &str,
    message: &str,
    author: &str,
    acting: &Acting,
) -> Result<(Imported, Vec<String>), SaveError> {
    let version = prepared("entries", src, message, author).map_err(SaveError::Invalid)?;
    let sections =
        super::operation::sections(manifest).map_err(|e| SaveError::Failed(e.to_string()))?;
    let snapshot = sections.snapshot.clone();
    let holds = format!("workspace save {}", author.escape_debug());
    let Acting {
        role,
        groups,
        grant,
        login,
        github_host,
    } = *acting;
    super::operation::locked(&sections, &holds, async move |valkey, config, _, held| {
        super::lock::holds(valkey, config, &held.token).await?;
        let store = Store::connect(&snapshot)?;
        let (mut version, mut owners) = (version, Vec::new());
        if role != Role::Operator {
            let Some(current) = current_entries(valkey, config, &snapshot).await? else {
                return Ok(Err(SaveError::RoleLost));
            };
            if authorize::role(role, groups, grant, &current) == Role::Viewer {
                return Ok(Err(SaveError::RoleLost));
            }
            let read: Vec<Json> = (current.iter())
                .filter(|e| grant.reads(authorize::repo(e)))
                .cloned()
                .collect();
            let submitted = &version.repositories;
            let seen = compose_entries(grant, login, github_host, &read, submitted);
            let refused = collisions(grant, &current, &seen, acting.refused(&read, &seen));
            if !refused.is_empty() {
                return Ok(Err(SaveError::Unauthorized(refused)));
            }
            let composed = compose_entries(grant, login, github_host, &current, submitted);
            owners = touched_owners(&current, &composed);
            let text = entries_text(&composed).ok_or_else(|| Msg::UiEntriesInvalid.text())?;
            version = match prepared("entries", &text, message, author) {
                Ok(version) => version,
                Err(problems) => return Ok(Err(SaveError::Invalid(problems))),
            };
        }
        let imported = store_version(&store, &snapshot, &version).await?;
        super::lock::holds(valkey, config, &held.token).await?;
        Ok(Ok((imported, owners)))
    })
    .map_err(|e| match OpError::refused(&*e) {
        true => SaveError::Refused(e.to_string()),
        false => SaveError::Failed(e.to_string()),
    })?
}

/// `refused`, the refusals of a team operator's save over the entries its grant reads, with each
/// entry of `seen` whose identity `current` holds outside the grant refused `outside_grant` as an
/// addition, whatever else it failed (#333): the browser sees such an identity as new, so only
/// the save can refuse it, named as the subject submitted it. In `seen` order, then the removals.
#[cfg(all(feature = "ui", feature = "refresh-lock"))]
fn collisions(
    grant: &Grant,
    current: &[Json],
    seen: &[Json],
    refused: Vec<Refusal>,
) -> Vec<Refusal> {
    let hidden: BTreeSet<&str> = (current.iter().map(authorize::repo))
        .filter(|repo| !grant.reads(repo))
        .collect();
    let (changed, removed): (Vec<_>, Vec<_>) =
        (refused.into_iter()).partition(|r| r.change != Change::Remove);
    let mut changed: BTreeMap<String, Refusal> = (changed.into_iter())
        .map(|r| (r.identity.clone(), r))
        .collect();
    let added = seen.iter().filter_map(|e| {
        let repo = authorize::repo(e);
        let found = changed.remove(repo);
        match hidden.contains(repo) {
            true => Some(Refusal {
                identity: repo.to_string(),
                change: Change::Add,
                reason: Reason::OutsideGrant,
            }),
            false => found,
        }
    });
    added.chain(removed).collect()
}

/// The id of the version the entries `src` would be stored as, or the problems of the checks
/// `import` and the console's save make first: what the audit records before the write.
pub fn version_id(src: &str, message: &str, author: &str) -> Result<String, Vec<String>> {
    prepared("entries", src, message, author).map(|v| v.id)
}

/// The entries of the version current in Valkey, checked; `None` when there is no pointer.
#[cfg(feature = "refresh-lock")]
async fn current_entries(
    valkey: &Valkey,
    config: &valkey::Config,
    snapshot: &Config,
) -> Result<Option<Vec<Json>>, String> {
    let Some((id, _)) = current(valkey, config).await? else {
        return Ok(None);
    };
    let store = Store::connect(snapshot)?;
    let key = version_key(snapshot, &id);
    let document = stored(&store, &key, &id).await?;
    Ok(Some(
        document
            .ok_or_else(|| Msg::WorkspaceNoVersion(&key).text())?
            .0
            .repositories,
    ))
}

/// The owners, sorted and without repeats, of the entries that differ between `current` and
/// `new`: the groups a team operator's change touched.
#[cfg(feature = "refresh-lock")]
fn touched_owners(current: &[Json], new: &[Json]) -> Vec<String> {
    let keyed = |entries: &[Json]| -> BTreeMap<String, Json> {
        (entries.iter())
            .map(|e| (authorize::repo(e).to_string(), e.clone()))
            .collect()
    };
    let (old, new) = (keyed(current), keyed(new));
    let touched = old.keys().chain(new.keys()).collect::<BTreeSet<_>>();
    (touched.into_iter())
        .filter(|repo| old.get(*repo) != new.get(*repo))
        .filter_map(|repo| new.get(repo).or_else(|| old.get(repo)))
        .filter_map(|entry| entry["owner"].as_str())
        .map(str::to_string)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

/// The version id of `entries`, their blocks being `blocks` (`entry_blocks`), and the entries as
/// a version document holds them.
fn versioned(entries: &[EntryFile], blocks: Vec<Json>) -> (String, Vec<Json>) {
    let ids: Vec<VersionEntry> = (entries.iter().zip(&blocks))
        .map(|(e, blocks)| {
            let (owner, added_by) = (e.owner.as_deref(), e.added_by.as_deref());
            (e.repo.as_str(), e.path.as_str(), blocks, owner, added_by)
        })
        .collect();
    let id = workspace_version_id(&ids);
    let repositories = (entries.iter().zip(blocks))
        .map(|(e, blocks)| entry(e, blocks))
        .collect();
    (id, repositories)
}

/// The version stored at `key`, checked as the version `id` (`checked`), with its entries in the
/// `[[repository]]` shape; `None` when there is none.
pub(crate) async fn stored(
    store: &Store,
    key: &str,
    id: &str,
) -> Result<Option<(Version, String)>, String> {
    match store.get(key, VERSION_MAX).await? {
        Some(bytes) => checked(key, id, &bytes).map(Some),
        None => Ok(None),
    }
}

/// The document `bytes`, stored at `key`, checked as the version `id` before any of it is used:
/// its id, its time's shape, its entries valid as a workspace's text (`meada_core::manifest`)
/// and the id derived from them. Returns it with its entries in the `[[repository]]` shape,
/// the text `import` reads back to the same id.
fn checked(key: &str, id: &str, bytes: &[u8]) -> Result<(Version, String), String> {
    let not = || Msg::WorkspaceNotVersion(key).text();
    let version: Version = serde_json::from_slice(bytes).map_err(|_| not())?;
    if version.id != id || !is_time(&version.time) {
        return Err(not());
    }
    let text = entries_text(&version.repositories).ok_or_else(not)?;
    let entries = validate(key, &text).map_err(|_| not())?;
    if versioned(&entries, entry_blocks(&text)).0 != id {
        return Err(not());
    }
    Ok((version, text))
}

/// Whether `text` is a time as a version records it: `YYYY-MM-DDTHH:MM:SSZ`.
pub(crate) fn is_time(text: &str) -> bool {
    let shape = "dddd-dd-ddTdd:dd:ddZ";
    text.len() == shape.len()
        && (text.bytes().zip(shape.bytes())).all(|(b, s)| {
            if s == b'd' {
                b.is_ascii_digit()
            } else {
                b == s
            }
        })
}

/// `meada workspace list`: every version in the store and the current one.
pub struct Listing {
    /// Newest first: by time, then by id.
    pub versions: Vec<Version>,
    /// Per stored document that failed its checks, the problem naming its key.
    pub malformed: Vec<String>,
    /// The current version's id and fence, or why none is.
    pub current: Result<(String, u64), String>,
}

impl Listing {
    /// One line per version, `<mark> <id> <time> <author>: <message>`, the current one marked
    /// `*`, then one `! <problem>` per malformed document. What the store holds reaches no
    /// terminal unescaped.
    pub fn text(&self) -> String {
        let current = self.current.as_ref().ok().map(|(id, _)| id.as_str());
        let versions = self.versions.iter().map(|v| {
            let mark = if current == Some(v.id.as_str()) {
                '*'
            } else {
                ' '
            };
            let (author, message) = (shown(&v.author), shown(&v.message));
            format!("{mark} {} {} {author}: {message}\n", v.id, v.time)
        });
        let malformed = self.malformed.iter().map(|m| format!("! {m}\n"));
        versions.chain(malformed).collect()
    }
}

/// `text` with its control characters escaped.
pub fn shown(text: &str) -> String {
    (text.chars())
        .map(|c| match c.is_control() {
            true => c.escape_default().to_string(),
            false => c.to_string(),
        })
        .collect()
}

/// `meada workspace list --workspace <manifest>`: the pointer is read and checked first (a
/// malformed one refuses the listing), then every document under `workspaces/`.
pub fn list(manifest: &Path) -> crate::Result<Listing> {
    let sections = sections(manifest)?;
    let run = async {
        let valkey = Valkey::connect(&sections.valkey).await?;
        let current = current(&valkey, &sections.valkey).await?;
        let store = Store::connect(&sections.snapshot)?;
        let (versions, malformed) = documents(&store, &sections.snapshot).await?;
        let no_current = || Msg::WorkspaceNoCurrent(&pointer_key(&sections.valkey)).text();
        Ok::<_, String>(Listing {
            versions,
            malformed,
            current: current.ok_or_else(no_current),
        })
    };
    Ok(crate::modules::block_on(run)??)
}

/// Every stored workspace version, checked and newest first, shared by the CLI and console.
pub(crate) async fn documents(
    store: &Store,
    snapshot: &Config,
) -> Result<(Vec<Version>, Vec<String>), String> {
    let prefix = snapshot.key("workspaces");
    let (mut versions, mut malformed) = (Vec::new(), Vec::new());
    for key in store.list(&prefix).await? {
        let id = key
            .strip_prefix(&format!("{prefix}/"))
            .and_then(|name| name.strip_suffix(".json"))
            .filter(|id| is_id(id));
        let Some(id) = id else {
            malformed.push(Msg::WorkspaceNotVersion(&shown(&key)).text());
            continue;
        };
        match stored(store, &key, id).await {
            Ok(Some((version, _))) => versions.push(version),
            Ok(None) => {}
            Err(problem) => malformed.push(shown(&problem)),
        }
    }
    versions.sort_by(|a, b| (&b.time, &a.id).cmp(&(&a.time, &b.id)));
    Ok((versions, malformed))
}

/// `meada workspace show [<version>] --workspace <manifest>`: the version `version`, or the
/// current one, as `[[repository]]` entries. The id given is checked first.
pub fn show(manifest: &Path, version: Option<&str>) -> crate::Result<String> {
    if let Some(id) = version.filter(|id| !is_id(id)) {
        return Err(Msg::BadVersionArg(&id.escape_debug().to_string())
            .text()
            .into());
    }
    let sections = sections(manifest)?;
    let run = async {
        let id = match version {
            Some(id) => id.to_string(),
            None => {
                let valkey = Valkey::connect(&sections.valkey).await?;
                let current = current(&valkey, &sections.valkey).await?;
                let no_current = || Msg::WorkspaceNoCurrent(&pointer_key(&sections.valkey)).text();
                current.ok_or_else(no_current)?.0
            }
        };
        let store = Store::connect(&sections.snapshot)?;
        let key = version_key(&sections.snapshot, &id);
        let stored = stored(&store, &key, &id).await?;
        let (_, text) = stored.ok_or_else(|| Msg::WorkspaceNoVersion(&key).text())?;
        Ok::<_, String>(text)
    };
    Ok(crate::modules::block_on(run)??)
}

/// The author of a version imported from the command line: `cli:<login>@<host>`, as the
/// operations name theirs.
#[cfg(feature = "refresh-lock")]
pub fn cli_author() -> String {
    super::lock::cli_author()
}

/// The author of a version imported from the command line: `cli:<login>@<host>`, from the
/// environment in a build without the refresh lock's system calls.
#[cfg(not(feature = "refresh-lock"))]
pub fn cli_author() -> String {
    let var = |names: [&str; 2]| names.iter().find_map(|n| std::env::var(n).ok());
    let login = var(["USER", "USERNAME"]).unwrap_or_default();
    let host = var(["HOSTNAME", "COMPUTERNAME"]).unwrap_or_default();
    format!("cli:{login}@{}", host.to_lowercase())
}

/// Why `apply` moved nothing, kept apart for the console's answer and audit.
#[cfg(feature = "refresh-lock")]
#[derive(Debug)]
pub enum ApplyError {
    /// The entries the subject may not change, as `authorize_entries` lists them.
    Refused(Vec<Refusal>),
    /// A team operator that the current version no longer makes one (`authorize::role`).
    RoleLost,
    /// An operation's own refusal: the version not stored, the lock held or lost, a fence not
    /// above the pointer's.
    Conflict(String),
    /// Anything else: the argument, the manifest, the store, Valkey or the pointer.
    Failed(String),
}

#[cfg(feature = "refresh-lock")]
impl ApplyError {
    /// Whether the audit records it as `refused` rather than `failed` (#328).
    pub fn refused(&self) -> bool {
        !matches!(self, ApplyError::Failed(_))
    }
}

#[cfg(feature = "refresh-lock")]
impl std::fmt::Display for ApplyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ApplyError::Refused(refused) => {
                f.write_str(&Msg::WorkspaceRefused(refused.len()).text())
            }
            ApplyError::RoleLost => f.write_str(&Msg::WorkspaceRoleLost.text()),
            ApplyError::Conflict(problem) | ApplyError::Failed(problem) => f.write_str(problem),
        }
    }
}

#[cfg(feature = "refresh-lock")]
impl std::error::Error for ApplyError {}

/// What `apply` did: the `<id> <fence>` line and, for a team operator, the owner groups of the
/// entries the change touched (none for the global operator).
#[cfg(feature = "refresh-lock")]
pub struct Applied {
    pub line: String,
    pub owners: Vec<String>,
}

/// `meada workspace apply <version> --workspace <manifest>` (feature `refresh-lock`): once the
/// store holds the version `id`'s document, checked, moves `{<key_prefix>}:workspace` to `id`
/// with the operation's fence under the refresh lock (`APPLY`); `<id> <fence>`. The id is
/// checked before anything is read. Anyone but the global operator (`acting`) is authorized
/// under the lock, before the pointer moves, against the version then current: its role
/// recomputed from it, then `authorize_entries` over its whole entries, so under the GitHub grant
/// provider an addition on its host without `added_by` is `unattributed`. The CLI passes the
/// global operator.
#[cfg(feature = "refresh-lock")]
pub fn apply(
    manifest: &Path,
    id: &str,
    author: &str,
    acting: &Acting,
) -> Result<Applied, ApplyError> {
    let Acting {
        role,
        groups,
        grant,
        ..
    } = *acting;
    fn failed(e: Box<dyn std::error::Error>) -> ApplyError {
        match OpError::refused(&*e) {
            true => ApplyError::Conflict(e.to_string()),
            false => ApplyError::Failed(e.to_string()),
        }
    }
    if !is_id(id) {
        let shown = id.escape_debug().to_string();
        return Err(ApplyError::Failed(Msg::BadVersionArg(&shown).text()));
    }
    let sections = super::operation::sections(manifest).map_err(failed)?;
    let key = version_key(&sections.snapshot, id);
    let check = async {
        let store = Store::connect(&sections.snapshot)?;
        match stored(&store, &key, id).await? {
            Some((version, _)) => Ok(version.repositories),
            None => Err(OpError::Refused(Msg::WorkspaceNoVersion(&key).text())),
        }
    };
    let entries = crate::modules::block_on(check)
        .map_err(|e| failed(e.into()))?
        .map_err(|e| failed(e.into()))?;
    let snapshot = &sections.snapshot;
    let done = super::operation::locked(
        &sections,
        &format!("workspace apply {}", author.escape_debug()),
        async |valkey, config, _, held| {
            let mut owners = Vec::new();
            if role != Role::Operator {
                let Some(current) = current_entries(valkey, config, snapshot).await? else {
                    return Ok(Err(ApplyError::RoleLost));
                };
                if authorize::role(role, groups, grant, &current) == Role::Viewer {
                    return Ok(Err(ApplyError::RoleLost));
                }
                let refused = acting.refused(&current, &entries);
                if !refused.is_empty() {
                    return Ok(Err(ApplyError::Refused(refused)));
                }
                owners = touched_owners(&current, &entries);
            }
            let (lock, pointer) = (config.key("lock", true), pointer_key(config));
            let mut script = cmd("EVAL");
            script.arg(APPLY).arg(2).arg(&lock).arg(&pointer);
            script.arg(&held.token).arg(id).arg(held.fence);
            let wrote = valkey.query::<i64>(&script).await;
            match wrote.map_err(|e| format!("{pointer}: {e}"))? {
                1 => Ok(Ok(Applied {
                    line: format!("{id} {}", held.fence),
                    owners,
                })),
                0 => Err(OpError::Refused(Msg::LockNotHeld(&lock).text())),
                -1 => Err(OpError::Refused(
                    Msg::FenceRefused {
                        fence: held.fence,
                        current: &pointer,
                        fence_key: &config.key("fence", true),
                    }
                    .text(),
                )),
                _ => Err(Msg::WorkspaceBadPointer(&pointer).text().into()),
            }
        },
    );
    done.map_err(failed)?
}

/// Points the workspace (`KEYS[2]`) at the version `ARGV[2]` with the fence `ARGV[3]` while the
/// lock (`KEYS[1]`) holds the token `ARGV[1]`: 0 without the lock, -1 unless the fence is above
/// the one the pointer holds (compared as decimal text), -2 when that one is no decimal; else
/// the hash set, and 1.
#[cfg(feature = "refresh-lock")]
const APPLY: &str = r"
if redis.call('GET', KEYS[1]) ~= ARGV[1] then
  return 0
end
local held = redis.call('HGET', KEYS[2], 'fence')
if held then
  if not string.match(held, '^%d+$') then
    return -2
  end
  held = (string.gsub(held, '^0+(%d)', '%1'))
  local fence = ARGV[3]
  if #fence < #held or (#fence == #held and fence <= held) then
    return -1
  end
end
redis.call('HSET', KEYS[2], 'id', ARGV[2], 'fence', ARGV[3])
return 1
";

/// What `meada workspace pull` did.
#[derive(Debug, PartialEq)]
pub enum Pulled {
    /// The current file now names this version.
    Swapped(String),
    /// The current file already named the pointer's version and fence: nothing changed.
    Current(String),
    /// The deployment workspace has no current version, with `[workspaces]` an empty deployment
    /// workspace: the current file is removed and only the contexts are pulled.
    Empty,
}

/// `meada workspace pull --workspace <manifest>`: the pointer `{<key_prefix>}:workspace` and the
/// version's document read and checked (`current`, `checked`) before anything on disk changes;
/// then, one pull at a time, the document written to `<index_dir>/workspace/<id>.json` and the
/// current file swapped by rename, never to a fence that is not above its own. Only the current
/// version's document and the one it replaced stay. A failure leaves the current file and the
/// documents as they were.
pub fn pull(manifest: &Path) -> crate::Result<Pulled> {
    let sections = sections(manifest)?;
    let pointer = pointer_key(&sections.valkey);
    // With `[workspaces]` a deployment with no current version is an empty deployment workspace.
    #[cfg(feature = "workspaces")]
    let empty_ok = sections.named.is_some();
    #[cfg(not(feature = "workspaces"))]
    let empty_ok = false;
    let fetch = async {
        let valkey = Valkey::connect(&sections.valkey).await?;
        let current = current(&valkey, &sections.valkey).await?;
        let store = Store::connect(&sections.snapshot)?;
        let deployment = match current {
            Some((id, fence)) => {
                let key = version_key(&sections.snapshot, &id);
                let bytes = store.get(&key, VERSION_MAX).await?;
                let bytes = bytes.ok_or_else(|| Msg::WorkspaceNoVersion(&key).text())?;
                checked(&key, &id, &bytes)?;
                Some((id, fence, bytes))
            }
            None if empty_ok => None,
            None => return Err(Msg::WorkspaceNoCurrent(&pointer).text()),
        };
        #[cfg(feature = "workspaces")]
        let contexts = match &sections.named {
            Some((postgres, _)) => {
                Some(named_contexts(postgres, &store, &sections.snapshot).await?)
            }
            None => None,
        };
        #[cfg(not(feature = "workspaces"))]
        let contexts: Option<()> = None;
        Ok::<_, String>((deployment, contexts))
    };
    let (deployment, contexts) = crate::modules::block_on(fetch)??;

    let dir = sections.index_dir.join(DIR);
    let at = pull::io_at(&dir);
    fs::create_dir_all(&dir).map_err(&at)?;
    let lock = (File::options().read(true).write(true).create(true))
        .truncate(false)
        .open(dir.join("pull.lock"))
        .map_err(&at)?;
    lock.lock().map_err(&at)?;
    // The contexts file first: a reader that sees the new version finds the contexts at least as
    // new, and the role check reads both before it writes the exclusions that name them.
    #[cfg(feature = "workspaces")]
    if let Some(contexts) = &contexts {
        crate::workspaces::write_contexts(&dir, contexts)?;
        eprintln!("meada: {}", Msg::ContextsWritten(contexts.len()).text());
    }
    #[cfg(not(feature = "workspaces"))]
    let _ = contexts;
    let Some((id, fence, bytes)) = deployment else {
        // The deployment workspace is empty: no current file, no document.
        let file = dir.join(CURRENT);
        if let Err(e) = fs::remove_file(&file)
            && e.kind() != std::io::ErrorKind::NotFound
        {
            return Err(pull::io_at(&file)(e).into());
        }
        return Ok(Pulled::Empty);
    };
    let file = dir.join(CURRENT);
    let previous = pull::pointer_file(&file, |shown| Msg::WorkspaceCurrentBad(shown).text())?;
    let document = dir.join(format!("{id}.json"));
    if let Some((held_id, held)) = &previous {
        if (held_id, *held) == (&id, fence) && document.is_file() {
            return Ok(Pulled::Current(id));
        }
        if fence < *held || (fence == *held && held_id != &id) {
            let file = file.display().to_string();
            return Err(Msg::WorkspaceFenceBelow {
                key: &pointer,
                id: &id,
                fence,
                held: *held,
                file: &file,
                fence_key: &sections.valkey.key("fence", true),
            }
            .text()
            .into());
        }
    }
    let had = document.is_file();
    let new = dir.join(format!("{id}.json.new"));
    let written = (fs::write(&new, &bytes).map_err(pull::io_at(&new)))
        .and_then(|()| fs::rename(&new, &document).map_err(pull::io_at(&document)))
        .and_then(|()| pull::swap(&dir, &id, fence));
    if let Err(e) = written {
        let _ = fs::remove_file(&new);
        if !had {
            let _ = fs::remove_file(&document);
        }
        return Err(e.into());
    }
    // Only the current version and the one it replaced; a failure is left for the next pull.
    let replaced = previous.map(|(id, _)| id);
    for entry in fs::read_dir(&dir).into_iter().flatten().flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        let stem = name.strip_suffix(".json").filter(|stem| is_id(stem));
        if stem.is_some_and(|stem| stem != id && Some(stem) != replaced.as_deref()) {
            let _ = fs::remove_file(entry.path());
        }
    }
    Ok(Pulled::Swapped(id))
}

/// Every named workspace not deleted with the entries of its current version, read in one
/// `REPEATABLE READ` transaction and from the store (hostile input, bounded as the deployment
/// version is): what the contexts file holds (#342).
#[cfg(feature = "workspaces")]
async fn named_contexts(
    postgres: &crate::postgres::Config,
    store: &Store,
    snapshot: &Config,
) -> Result<Vec<crate::workspaces::Context>, String> {
    let current = crate::workspaces::store::current_contexts(postgres).await?;
    let mut contexts = Vec::new();
    for (id, version, operators) in current {
        let hostile = || Msg::WorkspacesDocumentBad(&id).text();
        if !is_id(&id) || !is_id(&version) {
            return Err(hostile());
        }
        let key = snapshot.key(&format!("workspaces/{version}.json"));
        let bytes = store.get(&key, VERSION_MAX).await?;
        let bytes = bytes.ok_or_else(|| Msg::WorkspaceNoVersion(&key).text())?;
        let entries = crate::workspaces::store::document_entries(&key, &version, &bytes)?;
        contexts.push(crate::workspaces::Context {
            id,
            version,
            operators,
            entries,
        });
    }
    Ok(contexts)
}

/// The version the current file of `dir` (`<index_dir>/workspace/`) names, read from disk and
/// checked as a pull checks it: its id, its entries and their text in the `[[repository]]`
/// shape; `None` without a current file. No connection is opened.
pub fn pulled(dir: &Path) -> Result<Option<(String, Vec<EntryFile>, String)>, String> {
    let Some((id, _, text, shown)) = pulled_version(dir)? else {
        return Ok(None);
    };
    let entries = validate(&shown, &text).map_err(|_| Msg::WorkspaceNotVersion(&shown).text())?;
    Ok(Some((id, entries, text)))
}

/// The pulled version as `pulled` reads it: its id, its document, its entries' text and the path
/// it was read from.
fn pulled_version(dir: &Path) -> Result<Option<(String, Version, String, String)>, String> {
    let file = dir.join(CURRENT);
    let current = pull::pointer_file(&file, |shown| Msg::WorkspaceCurrentBad(shown).text())?;
    let Some((id, _)) = current else {
        return Ok(None);
    };
    let path = dir.join(format!("{id}.json"));
    let shown = path.display().to_string();
    let bytes = crate::config::read_bounded(&path, VERSION_MAX)?;
    let (version, text) = checked(&shown, &id, &bytes)?;
    Ok(Some((id, version, text, shown)))
}

/// `pulled` without the entries the exclusions file of `dir` names (`excluded`): what every
/// reader of the pulled version reads under `[oidc.github]` with `source = "store"` (#331), an
/// excluded identity being absent from the workspace. `meada github-app roles` alone reads the
/// version whole.
pub fn eligible(dir: &Path) -> Result<Option<(String, Vec<EntryFile>, String)>, String> {
    let Some((id, version, _, shown)) = pulled_version(dir)? else {
        return Ok(None);
    };
    let excluded = excluded(dir, &id)?;
    let kept: Vec<Json> = (version.repositories.into_iter())
        .filter(|e| {
            !e["repo"]
                .as_str()
                .is_some_and(|repo| excluded.contains(repo))
        })
        .collect();
    let not = || Msg::WorkspaceNotVersion(&shown).text();
    let text = entries_text(&kept).ok_or_else(not)?;
    let entries = validate(&shown, &text).map_err(|_| not())?;
    Ok(Some((id, entries, text)))
}

/// The exclusions file in `DIR` (#331): the version id `meada github-app roles` checked on its
/// first line, then the identities it left out, sorted, one per line. With `[workspaces]` (#342)
/// a line `# contexts <digest>` follows the version, naming the contexts file the file was
/// decided for, and each named workspace's exclusion is `<context id>\t<identity>` after the
/// identities.
pub const EXCLUDED: &str = "excluded";

// ponytail: the most bytes of the exclusions file: one line per excluded (workspace, identity),
// at most `max_workspaces` of 2000 workspaces of `max_entries` of 500 entries each.
const EXCLUDED_MAX: u64 = 128 << 20;

/// The version id the exclusions file names for a deployment workspace with no current version
/// (an empty deployment under `[workspaces]`, #342).
pub const NO_VERSION: &str = "0000000000000000";

/// The prefix of the exclusions file's line naming the contexts file it was written for (#342).
const CONTEXTS_LINE: &str = "# contexts ";

/// The identities the exclusions file of `dir` leaves out of the pulled version `version`. The
/// file is hostile: refused, naming `meada github-app roles`, when it is missing or names another
/// version id, and naming its line when one is not a version id or an identity.
pub fn excluded(dir: &Path, version: &str) -> Result<BTreeSet<String>, String> {
    parse_excluded(dir, version).map(|(deployment, _, _)| deployment)
}

/// The identities the exclusions file leaves out of each named workspace (#342), by context id:
/// `excluded`'s checks, and the file must name the contexts file `digest` it was written for.
#[cfg(feature = "workspaces")]
pub fn excluded_contexts(
    dir: &Path,
    version: &str,
    digest: &str,
) -> Result<std::collections::BTreeMap<String, BTreeSet<String>>, String> {
    let (_, contexts, named) = parse_excluded(dir, version)?;
    match named.as_deref() == Some(digest) {
        true => Ok(contexts),
        false => Err(Msg::ContextsOther(&dir.join(EXCLUDED).display().to_string()).text()),
    }
}

/// The exclusions file parsed: the deployment workspace's identities, each context's (a line
/// `<context id>\t<identity>`) and the digest of the contexts file it names, if any.
#[allow(clippy::type_complexity)]
fn parse_excluded(
    dir: &Path,
    version: &str,
) -> Result<
    (
        BTreeSet<String>,
        std::collections::BTreeMap<String, BTreeSet<String>>,
        Option<String>,
    ),
    String,
> {
    let file = dir.join(EXCLUDED);
    let shown = file.display().to_string();
    if fs::symlink_metadata(&file).is_err_and(|e| e.kind() == std::io::ErrorKind::NotFound) {
        return Err(Msg::ExcludedMissing(&shown).text());
    }
    let bytes = crate::config::read_bounded(&file, EXCLUDED_MAX)?;
    let bad = |line| Msg::ExcludedBad { file: &shown, line }.text();
    let text = String::from_utf8(bytes).map_err(|_| bad(1))?;
    let mut lines = text.lines();
    let named = lines.next().filter(|id| is_id(id)).ok_or_else(|| bad(1))?;
    if named != version {
        let (file, pulled) = (shown.as_str(), version);
        return Err(Msg::ExcludedOther {
            file,
            named,
            pulled,
        }
        .text());
    }
    let identity = |line: &str| {
        !line.is_empty()
            && !line.chars().any(|c| c.is_control() || c.is_whitespace())
            && meada_core::manifest::identity_problem(line).is_none()
    };
    let (mut deployment, mut contexts, mut digest) =
        (BTreeSet::new(), std::collections::BTreeMap::new(), None);
    for (n, line) in lines.enumerate() {
        let at = n + 2;
        if let Some(named) = line.strip_prefix(CONTEXTS_LINE) {
            if !is_id(named) || digest.replace(named.to_string()).is_some() {
                return Err(bad(at));
            }
        } else if let Some((context, repo)) = line.split_once('\t') {
            if !is_id(context) || !identity(repo) {
                return Err(bad(at));
            }
            contexts
                .entry(context.to_string())
                .or_insert_with(BTreeSet::new)
                .insert(repo.to_string());
        } else if identity(line) {
            deployment.insert(line.to_string());
        } else {
            return Err(bad(at));
        }
    }
    Ok((deployment, contexts, digest))
}

/// The refusal of a reader of `manifest` with no version pulled into `dir`, naming the pull.
pub(crate) fn not_pulled(manifest: &Path, dir: &Path) -> String {
    let (manifest, dir) = (manifest.display().to_string(), dir.display().to_string());
    Msg::WorkspaceNotPulled {
        manifest: &manifest,
        dir: &dir,
    }
    .text()
}

/// Removes the exclusions file of `dir`, if there is one.
pub fn remove_excluded(dir: &Path) -> Result<(), String> {
    let file = dir.join(EXCLUDED);
    match fs::remove_file(&file) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(pull::io_at(&file)(e)),
        _ => Ok(()),
    }
}

/// `<dir>/excluded.lock`, taken (waiting for another run's): what a role check holds from before
/// its commit until it has written the file.
pub fn lock_excluded(dir: &Path) -> Result<File, String> {
    let path = dir.join(format!("{EXCLUDED}.lock"));
    let at = pull::io_at(&path);
    let lock = (File::options().read(true).write(true).create(true))
        .truncate(false)
        .open(&path)
        .map_err(&at)?;
    lock.lock().map_err(&at)?;
    Ok(lock)
}

/// Writes the exclusions file of `dir` by rename: `version`, then `identities` in order.
pub fn write_excluded(
    dir: &Path,
    version: &str,
    identities: &BTreeSet<String>,
) -> Result<(), String> {
    write_excluded_with(dir, version, identities, None)
}

/// `write_excluded` with the named workspaces' exclusions (#342): a line `# contexts <digest>`
/// naming the contexts file they were decided for, then each `(context id, identity)` as
/// `<context id>\t<identity>`, sorted.
pub fn write_excluded_with(
    dir: &Path,
    version: &str,
    identities: &BTreeSet<String>,
    contexts: Option<(&str, &BTreeSet<(String, String)>)>,
) -> Result<(), String> {
    let mut lines = vec![version.to_string()];
    if let Some((digest, _)) = contexts {
        lines.push(format!("{CONTEXTS_LINE}{digest}"));
    }
    lines.extend(identities.iter().cloned());
    if let Some((_, named)) = contexts {
        lines.extend(
            named
                .iter()
                .map(|(context, repo)| format!("{context}\t{repo}")),
        );
    }
    let text: String = lines.into_iter().map(|line| format!("{line}\n")).collect();
    let (file, new) = (dir.join(EXCLUDED), dir.join(format!("{EXCLUDED}.new")));
    let written = (fs::write(&new, text).map_err(pull::io_at(&new)))
        .and_then(|()| fs::rename(&new, &file).map_err(pull::io_at(&file)));
    if written.is_err() {
        let _ = fs::remove_file(&new);
    }
    written
}

/// `meada workspace show --pulled --workspace <manifest>`: the version `meada workspace pull` left
/// on disk, in the `[[repository]]` shape, for the fetch script; under `[oidc.github]` with
/// `source = "store"` without the entries the role check excluded, refused as the readers refuse.
/// No connection is opened.
pub fn show_pulled(manifest: &Path) -> crate::Result<String> {
    let read = Manifest::read(manifest)?;
    let dir = read.index_dir.join(DIR);
    let found = match read.excludes() {
        true => eligible(&dir)?,
        false => pulled(&dir)?,
    };
    #[cfg(feature = "workspaces")]
    if let Some(section) = &read.workspaces {
        // The repositories only named workspaces select, with the paths the fetch script
        // provisions them at (ADR 0021): after the deployment workspace's own entries.
        let (version, deployment, text) = match found {
            Some((version, entries, text)) => (version, entries, text),
            None => (String::new(), Vec::new(), String::new()),
        };
        let identities: Vec<String> = deployment.iter().map(|e| e.repo.clone()).collect();
        let membership = crate::workspaces::membership(&dir, &version, &identities)?;
        let mut text = text;
        for identity in membership.members {
            let path = crate::workspaces::member_path(&section.mirrors, &identity);
            let sep = if text.is_empty() || text.ends_with("\n\n") {
                ""
            } else {
                "\n"
            };
            text = format!(
                "{text}{sep}[[repository]]\nrepo = {}\npath = {}\n",
                toml::Value::String(identity),
                toml::Value::String(path.display().to_string()),
            );
        }
        return Ok(text);
    }
    Ok(found.ok_or_else(|| not_pulled(manifest, &dir))?.2)
}

#[cfg(all(test, feature = "ui", feature = "refresh-lock"))]
mod tests {
    use super::*;
    use serde_json::json;

    /// An exclusions file past its bound is refused unread, and a context line must name an id
    /// and an identity.
    #[test]
    fn the_exclusions_file_is_bounded_and_its_context_lines_are_checked() {
        let dir = std::env::temp_dir().join(format!("meada-excluded-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let version = "0123456789abcdef";
        let digest = "fedcba9876543210";
        let write = |text: String| fs::write(dir.join(EXCLUDED), text).unwrap();
        write(format!(
            "{version}\n{CONTEXTS_LINE}{digest}\nh/o/a\n{version}\th/o/b\n"
        ));
        assert_eq!(excluded(&dir, version).unwrap().len(), 1);
        #[cfg(feature = "workspaces")]
        {
            let named = excluded_contexts(&dir, version, digest).unwrap();
            assert_eq!(named[version].len(), 1);
            assert!(excluded_contexts(&dir, version, version).is_err());
        }
        for bad in [
            "nope\th/o/b\n",
            "0123456789abcdef\t\n",
            "0123456789abcdef\t../x\n",
        ] {
            write(format!("{version}\n{bad}"));
            assert!(excluded(&dir, version).is_err(), "{bad:?}");
        }
        let big = fs::File::create(dir.join(EXCLUDED)).unwrap();
        big.set_len(EXCLUDED_MAX + 1).unwrap();
        assert!(excluded(&dir, version).is_err());
    }

    /// An identity the version holds outside the grant is `outside_grant` whatever the core
    /// found, an attribution missing included, in the submitted order and before the removals.
    #[test]
    fn a_collision_with_a_hidden_identity_is_outside_grant_first() {
        let entry = |repo: &str, owner: &str| json!({"repo": repo, "path": "p", "owner": owner});
        let grant = Grant::Only(["g.com/a".into(), "g.com/b".into(), "g.com/r".into()].into());
        let current = [
            entry("g.com/a", "team-a"),
            entry("g.com/hidden", "team-a"),
            entry("g.com/r", "team-b"),
        ];
        let read: Vec<Json> = (current.iter())
            .filter(|e| grant.reads(authorize::repo(e)))
            .cloned()
            .collect();
        let seen = [
            entry("g.com/b", "team-b"),
            entry("g.com/hidden", "team-a"),
            entry("g.com/a", "team-a"),
            entry("g.com/new", "team-a"),
        ];
        let groups = ["team-a".to_string()];
        let acting = Acting {
            role: Role::TeamOperator,
            groups: &groups,
            grant: &grant,
            login: None,
            github_host: Some("g.com"),
        };
        let refused = acting.refused(&read, &seen);
        let reasons = |refused: &[Refusal]| -> Vec<(String, Reason)> {
            (refused.iter())
                .map(|r| (r.identity.clone(), r.reason))
                .collect()
        };
        let unattributed = |repo: &str| (repo.to_string(), Reason::Unattributed);
        assert_eq!(
            reasons(&refused),
            [
                unattributed("g.com/b"),
                unattributed("g.com/hidden"),
                unattributed("g.com/new"),
                ("g.com/r".into(), Reason::NotYourTeam),
            ]
        );
        let refused = collisions(&grant, &current, &seen, refused);
        assert_eq!(
            reasons(&refused),
            [
                unattributed("g.com/b"),
                ("g.com/hidden".into(), Reason::OutsideGrant),
                unattributed("g.com/new"),
                ("g.com/r".into(), Reason::NotYourTeam),
            ]
        );
        assert_eq!(refused[1].change, Change::Add);
        // Without a refusal of its own, a collision is still refused.
        let acting = Acting {
            login: Some("ana"),
            ..acting
        };
        let mut attributed = seen.clone();
        attributed[1]["added_by"] = "ana".into();
        let refused = acting.refused(&read, &attributed[1..2]);
        assert!(refused.iter().all(|r| r.identity != "g.com/hidden"));
        let refused = collisions(&grant, &current, &attributed[1..2], refused);
        assert_eq!(
            reasons(&refused)[0],
            ("g.com/hidden".into(), Reason::OutsideGrant)
        );
    }
}
