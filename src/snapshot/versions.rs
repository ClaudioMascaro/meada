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

use std::fs::{self, File};
use std::path::{Path, PathBuf};

#[cfg(feature = "refresh-lock")]
use meada_core::authorize::{self, Grant, Refusal, Role, authorize_entries};
use meada_core::ids::workspace_version_id;
use meada_core::manifest::{EntryFile, validate, validate_import};
use meada_core::workspace::entries_text;
use redis::cmd;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value as Json};

use super::generations::{now, rfc3339};
use super::store::Store;
use super::{Config, entry_blocks, fence, is_id, pull};
use crate::msg::Msg;
use crate::valkey::{self, Valkey};
use crate::workspace::Manifest;

// ponytail: the most a workspace version document may hold in the store; past it the document
// is refused, naming its key.
const VERSION_MAX: u64 = 4 << 20;

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
    Refused(String),
}

/// The console's save is CLI import's text preparation and storage under the refresh lock.
/// CLI import deliberately remains available with generation-pointer alone and takes no lock.
#[cfg(all(feature = "ui", feature = "refresh-lock"))]
pub(crate) fn save(
    manifest: &Path,
    src: &str,
    message: &str,
    author: &str,
) -> Result<Imported, SaveError> {
    let version = prepared("entries", src, message, author).map_err(SaveError::Invalid)?;
    let sections =
        super::operation::sections(manifest).map_err(|e| SaveError::Refused(e.to_string()))?;
    let snapshot = sections.snapshot.clone();
    let holds = format!("workspace save {}", author.escape_debug());
    super::operation::locked(&sections, &holds, async move |valkey, config, _, held| {
        super::lock::holds(valkey, config, &held.token).await?;
        let store = Store::connect(&snapshot)?;
        let imported = store_version(&store, &snapshot, &version).await?;
        super::lock::holds(valkey, config, &held.token).await?;
        Ok(imported)
    })
    .map_err(|e| SaveError::Refused(e.to_string()))
}

/// One entry of a version document: `repo` and `path`, the `owner` when there is one, then the
/// entry's blocks.
fn entry(e: &EntryFile, blocks: Json) -> Json {
    let mut entry = Map::new();
    entry.insert("repo".into(), e.repo.as_str().into());
    entry.insert("path".into(), e.path.as_str().into());
    if let Some(owner) = &e.owner {
        entry.insert("owner".into(), owner.as_str().into());
    }
    if let Json::Object(blocks) = blocks {
        entry.extend(blocks);
    }
    Json::Object(entry)
}

/// The version id of `entries`, their blocks being `blocks` (`entry_blocks`), and the entries as
/// a version document holds them.
fn versioned(entries: &[EntryFile], blocks: Vec<Json>) -> (String, Vec<Json>) {
    let ids: Vec<(&str, &str, &Json, Option<&str>)> = (entries.iter().zip(&blocks))
        .map(|(e, blocks)| (e.repo.as_str(), e.path.as_str(), blocks, e.owner.as_deref()))
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
    /// Anything else: the argument, the manifest, the store, Valkey, the lock or the pointer.
    Failed(String),
}

#[cfg(feature = "refresh-lock")]
impl std::fmt::Display for ApplyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ApplyError::Refused(refused) => {
                f.write_str(&Msg::WorkspaceRefused(refused.len()).text())
            }
            ApplyError::RoleLost => f.write_str(&Msg::WorkspaceRoleLost.text()),
            ApplyError::Failed(problem) => f.write_str(problem),
        }
    }
}

#[cfg(feature = "refresh-lock")]
impl std::error::Error for ApplyError {}

/// `meada workspace apply <version> --workspace <manifest>` (feature `refresh-lock`): once the
/// store holds the version `id`'s document, checked, moves `{<key_prefix>}:workspace` to `id`
/// with the operation's fence under the refresh lock (`APPLY`); `<id> <fence>`. The id is
/// checked before anything is read. Anyone but the global operator (`role`, its `groups` and
/// `grant`) is authorized under the lock, before the pointer moves, against the version then
/// current: its role recomputed from it, then `authorize_entries` over its whole entries. The
/// CLI passes the global operator.
#[cfg(feature = "refresh-lock")]
pub fn apply(
    manifest: &Path,
    id: &str,
    author: &str,
    role: Role,
    groups: &[String],
    grant: &Grant,
) -> Result<String, ApplyError> {
    fn failed(e: impl std::fmt::Display) -> ApplyError {
        ApplyError::Failed(e.to_string())
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
            None => Err(Msg::WorkspaceNoVersion(&key).text()),
        }
    };
    let entries = crate::modules::block_on(check)
        .map_err(failed)?
        .map_err(ApplyError::Failed)?;
    let snapshot = &sections.snapshot;
    let done = super::operation::locked(
        &sections,
        &format!("workspace apply {}", author.escape_debug()),
        async |valkey, config, _, held| {
            if role != Role::Operator {
                let current = match current(valkey, config).await? {
                    Some((current_id, _)) => {
                        let store = Store::connect(snapshot)?;
                        let key = version_key(snapshot, &current_id);
                        let document = stored(&store, &key, &current_id).await?;
                        document
                            .ok_or_else(|| Msg::WorkspaceNoVersion(&key).text())?
                            .0
                    }
                    None => return Ok(Err(ApplyError::RoleLost)),
                };
                let current = current.repositories;
                if authorize::role(role, groups, grant, &current) == Role::Viewer {
                    return Ok(Err(ApplyError::RoleLost));
                }
                let refused = authorize_entries(role, groups, grant, &current, &entries);
                if !refused.is_empty() {
                    return Ok(Err(ApplyError::Refused(refused)));
                }
            }
            let (lock, pointer) = (config.key("lock", true), pointer_key(config));
            let mut script = cmd("EVAL");
            script.arg(APPLY).arg(2).arg(&lock).arg(&pointer);
            script.arg(&held.token).arg(id).arg(held.fence);
            let wrote = valkey.query::<i64>(&script).await;
            match wrote.map_err(|e| format!("{pointer}: {e}"))? {
                1 => Ok(Ok(format!("{id} {}", held.fence))),
                0 => Err(Msg::LockNotHeld(&lock).text()),
                -1 => Err(Msg::FenceRefused {
                    fence: held.fence,
                    current: &pointer,
                    fence_key: &config.key("fence", true),
                }
                .text()),
                _ => Err(Msg::WorkspaceBadPointer(&pointer).text()),
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
    let fetch = async {
        let valkey = Valkey::connect(&sections.valkey).await?;
        let current = current(&valkey, &sections.valkey).await?;
        let (id, fence) = current.ok_or_else(|| Msg::WorkspaceNoCurrent(&pointer).text())?;
        let store = Store::connect(&sections.snapshot)?;
        let key = version_key(&sections.snapshot, &id);
        let bytes = store.get(&key, VERSION_MAX).await?;
        let bytes = bytes.ok_or_else(|| Msg::WorkspaceNoVersion(&key).text())?;
        checked(&key, &id, &bytes)?;
        Ok::<_, String>((id, fence, bytes))
    };
    let (id, fence, bytes) = crate::modules::block_on(fetch)??;

    let dir = sections.index_dir.join(DIR);
    let at = pull::io_at(&dir);
    fs::create_dir_all(&dir).map_err(&at)?;
    let lock = (File::options().read(true).write(true).create(true))
        .truncate(false)
        .open(dir.join("pull.lock"))
        .map_err(&at)?;
    lock.lock().map_err(&at)?;
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

/// The version the current file of `dir` (`<index_dir>/workspace/`) names, read from disk and
/// checked as a pull checks it: its id, its entries and their text in the `[[repository]]`
/// shape; `None` without a current file. No connection is opened.
pub fn pulled(dir: &Path) -> Result<Option<(String, Vec<EntryFile>, String)>, String> {
    let file = dir.join(CURRENT);
    let current = pull::pointer_file(&file, |shown| Msg::WorkspaceCurrentBad(shown).text())?;
    let Some((id, _)) = current else {
        return Ok(None);
    };
    let path = dir.join(format!("{id}.json"));
    let shown = path.display().to_string();
    let bytes = crate::config::read_bounded(&path, VERSION_MAX)?;
    let (_, text) = checked(&shown, &id, &bytes)?;
    let entries = validate(&shown, &text).map_err(|_| Msg::WorkspaceNotVersion(&shown).text())?;
    Ok(Some((id, entries, text)))
}
