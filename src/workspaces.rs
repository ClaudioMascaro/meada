//! Named workspaces (spec S5, ADR 0020 and 0021, feature `workspaces`): the `[workspaces]` section
//! and the contexts the job reads. A named workspace is a named query context over the shared
//! indexing of a deployment: it selects repositories and describes the links between them, and
//! changes nothing about how a repository is indexed or published. This file holds what the
//! readers on disk share (the section, the contexts file `meada workspace pull` writes and the
//! membership it gives); the Postgres records are `workspaces/store.rs`, the console's routes
//! `ui/workspaces.rs`.

use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde_json::{Value as Json, json};

use crate::modules::Problem;
use crate::msg::Msg;

pub mod cli;
pub(crate) mod lifecycle;
pub(crate) mod store;

use lifecycle::OPERATORS_MAX;

// ponytail: the most workspaces `max_workspaces` may allow, and its default (ADR 0021).
const WORKSPACES_MAX: i64 = 2000;
const WORKSPACES_DEFAULT: i64 = 200;
// ponytail: the most entries `max_entries` may allow, and its default; also what a reader of
// the contexts file or a version document accepts, whatever `max_entries` says now.
pub(crate) const ENTRIES_MAX: i64 = 500;
const ENTRIES_DEFAULT: i64 = 100;

/// `[workspaces]` as written.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct File {
    mirrors: Option<String>,
    max_workspaces: Option<i64>,
    max_entries: Option<i64>,
}

/// The validated section.
#[derive(Clone, Debug, PartialEq)]
pub struct Config {
    /// The root of the mirrors of the repositories only named workspaces select: a member lives at
    /// `<mirrors>/<identity>`.
    pub mirrors: PathBuf,
    pub max_workspaces: usize,
    pub max_entries: usize,
}

impl File {
    /// The section checked: the first problem.
    pub fn validate(self) -> Result<Config, Problem> {
        let Some(mirrors) = self.mirrors else {
            return Err(("workspaces", Msg::KeyMissing("workspaces.mirrors").text()));
        };
        let mirrors = PathBuf::from(mirrors);
        if !mirrors.is_absolute() || mirrors.components().any(|c| c.as_os_str() == "..") {
            return Err(("workspaces.mirrors", Msg::WorkspacesMirrors.text()));
        }
        let bounded = |key: &'static str, value, default, max: i64| {
            let n = crate::modules::at_least(key, value, default, 1)?;
            match i64::try_from(n) {
                Ok(n) if n <= max => Ok(n as usize),
                _ => Err((key, Msg::AtMost(max as u64).text())),
            }
        };
        Ok(Config {
            mirrors,
            max_workspaces: bounded(
                "workspaces.max_workspaces",
                self.max_workspaces,
                WORKSPACES_DEFAULT,
                WORKSPACES_MAX,
            )?,
            max_entries: bounded(
                "workspaces.max_entries",
                self.max_entries,
                ENTRIES_DEFAULT,
                ENTRIES_MAX,
            )?,
        })
    }
}

/// The contexts file in `<index_dir>/workspace/` (ADR 0021): what `meada workspace pull` read
/// from Postgres and the store in one transaction, which every other reader takes from disk.
pub const CONTEXTS: &str = "contexts";

// ponytail: the most bytes of the contexts file (`max_workspaces` of 2000 workspaces of
// `max_entries` of 500 entries each, with room for their blocks); past it the file is refused.
const CONTEXTS_MAX: u64 = 256 << 20;

/// One named workspace as the job reads it: its stable id, the version it is at, the logins of
/// its operators (the sponsors of its entries) and the entries of that version.
#[derive(Clone, Debug, PartialEq)]
pub struct Context {
    pub id: String,
    pub version: String,
    pub operators: Vec<String>,
    /// The version document's entries, in manifest order: `{"repo": ..., blocks...}`.
    pub entries: Vec<Json>,
    /// The identities of the version the sponsor rule excluded from `entries`, sorted: what
    /// `membership` removed (#510). Empty as the contexts file holds a context.
    pub excluded: Vec<String>,
}

impl Context {
    pub fn identities(&self) -> Vec<String> {
        meada_core::named::identities(&self.entries)
    }

    /// The entry's blocks as a generation records them (`entry_blocks`): the entry without `repo`.
    pub fn blocks(entry: &Json) -> Json {
        let mut blocks = entry.clone();
        if let Some(map) = blocks.as_object_mut() {
            map.remove("repo");
        }
        blocks
    }
}

/// The contexts file's bytes for `contexts`, sorted by id: the JSON the readers check.
pub fn contexts_text(contexts: &[Context]) -> String {
    let mut sorted: Vec<&Context> = contexts.iter().collect();
    sorted.sort_by(|a, b| a.id.cmp(&b.id));
    let list: Vec<Json> = (sorted.iter())
        .map(|c| {
            json!({
                "id": c.id,
                "version": c.version,
                "operators": c.operators,
                "repositories": c.entries,
            })
        })
        .collect();
    let mut text = json!({"contexts": list}).to_string();
    text.push('\n');
    text
}

/// The digest of the contexts file's bytes: the first 16 hex characters of their BLAKE3 hash,
/// which the exclusions file names so that a reader never combines two pulls.
pub fn contexts_digest(text: &str) -> String {
    blake3::hash(text.as_bytes()).to_hex()[..16].to_string()
}

/// The contexts file of `dir` (`<index_dir>/workspace/`), checked: hostile input, so every id,
/// operator login, identity and entry is validated again, and a file that fails is refused,
/// naming it. `None`
/// when the file is missing, with the digest of its bytes otherwise.
pub fn read_contexts(dir: &Path) -> Result<Option<(Vec<Context>, String)>, String> {
    let file = dir.join(CONTEXTS);
    let shown = file.display().to_string();
    if std::fs::symlink_metadata(&file).is_err_and(|e| e.kind() == std::io::ErrorKind::NotFound) {
        return Ok(None);
    }
    let bytes = crate::config::read_bounded(&file, CONTEXTS_MAX)?;
    let bad = || Msg::ContextsBad(&shown).text();
    let text = String::from_utf8(bytes).map_err(|_| bad())?;
    let parsed: Json = serde_json::from_str(&text).map_err(|_| bad())?;
    let list = parsed["contexts"].as_array().ok_or_else(bad)?;
    let mut contexts: Vec<Context> = Vec::new();
    for item in list {
        let id = item["id"].as_str().filter(|i| meada_core::ids::is_id(i));
        let version = item["version"]
            .as_str()
            .filter(|v| meada_core::ids::is_id(v));
        let (Some(id), Some(version)) = (id, version) else {
            return Err(bad());
        };
        if contexts.last().is_some_and(|last| last.id.as_str() >= id) {
            return Err(bad());
        }
        let operators: Option<Vec<String>> = (item["operators"].as_array())
            .and_then(|a| a.iter().map(|o| o.as_str().map(String::from)).collect());
        let entries = item["repositories"].as_array().cloned();
        let (Some(operators), Some(entries)) = (operators, entries) else {
            return Err(bad());
        };
        let login_ok = |l: &String| meada_core::manifest::added_by_problem(l).is_none();
        if entries.len() > ENTRIES_MAX as usize
            || operators.len() > OPERATORS_MAX
            || !operators.iter().all(login_ok)
        {
            return Err(bad());
        }
        // The entries are a version document's: the text they make validates as a named text.
        let text = meada_core::workspace::entries_text(&entries).ok_or_else(bad)?;
        let checked = meada_core::named::version_entries(&shown, &text, ENTRIES_MAX as usize);
        let checked = checked.map_err(|_| bad())?;
        if meada_core::named::version_id(&checked) != version {
            return Err(bad());
        }
        contexts.push(Context {
            id: id.to_string(),
            version: version.to_string(),
            operators,
            entries: checked,
            excluded: Vec::new(),
        });
    }
    let digest = contexts_digest(&text);
    Ok(Some((contexts, digest)))
}

/// Writes the contexts file of `dir` by rename; its digest.
pub fn write_contexts(dir: &Path, contexts: &[Context]) -> Result<String, String> {
    let text = contexts_text(contexts);
    let (file, new) = (dir.join(CONTEXTS), dir.join(format!("{CONTEXTS}.new")));
    let at = crate::snapshot::pull::io_at;
    let written = (std::fs::write(&new, &text).map_err(at(&new)))
        .and_then(|()| std::fs::rename(&new, &file).map_err(at(&file)));
    if written.is_err() {
        let _ = std::fs::remove_file(&new);
    }
    written.map(|()| contexts_digest(&text))
}

/// What the pulled contexts and the role check's exclusions decide (ADR 0021): the named
/// workspaces with the entries the sponsor rule left in and the ones it excluded (dropping the
/// workspaces left empty), and the repositories only they select, sorted by identity.
pub struct Membership {
    pub contexts: Vec<Context>,
    pub members: Vec<String>,
}

/// The membership of the pulled state in `dir` (`<index_dir>/workspace/`) beside the deployment
/// workspace's `deployment` identities (after its own exclusions), `version` being the pulled
/// deployment version, empty for an empty deployment workspace. Reads only files: the contexts
/// file and the exclusions file, which must name it.
pub fn membership(dir: &Path, version: &str, deployment: &[String]) -> Result<Membership, String> {
    let (named, digest) = read_contexts(dir)?
        .ok_or_else(|| Msg::ContextsMissing(&dir.display().to_string()).text())?;
    let version = match version.is_empty() {
        true => crate::snapshot::versions::NO_VERSION,
        false => version,
    };
    let out = crate::snapshot::versions::excluded_contexts(dir, version, &digest)?;
    let mut members = std::collections::BTreeSet::new();
    let mut contexts = Vec::new();
    for mut context in named {
        let left = out.get(&context.id);
        let mut excluded = std::collections::BTreeSet::new();
        context.entries.retain(|e| {
            let repo = e["repo"].as_str().unwrap_or_default();
            let dropped = left.is_some_and(|l| l.contains(repo));
            if dropped {
                excluded.insert(repo.to_string());
            }
            !dropped
        });
        context.excluded = excluded.into_iter().collect();
        if context.entries.is_empty() {
            continue;
        }
        for identity in context.identities() {
            if !deployment.contains(&identity) {
                members.insert(identity);
            }
        }
        contexts.push(context);
    }
    Ok(Membership {
        contexts,
        members: members.into_iter().collect(),
    })
}

/// The directory where an identity only named workspaces select is mirrored.
pub fn member_path(mirrors: &Path, identity: &str) -> PathBuf {
    mirrors.join(identity)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn section(toml: &str) -> Result<Config, Problem> {
        toml::from_str::<File>(toml).unwrap().validate()
    }

    #[test]
    fn the_section_needs_an_absolute_mirrors_root_and_bounds_its_limits() {
        let config = section("mirrors = \"/srv/mirrors\"").unwrap();
        assert_eq!((config.max_workspaces, config.max_entries), (200, 100));
        let key = |toml: &str| section(toml).unwrap_err().0;
        assert_eq!(key(""), "workspaces");
        assert_eq!(key("mirrors = \"relative\""), "workspaces.mirrors");
        assert_eq!(key("mirrors = \"/a/../b\""), "workspaces.mirrors");
        for (limit, bad) in [("max_workspaces", 2001), ("max_entries", 501)] {
            let toml = format!("mirrors = \"/m\"\n{limit} = {bad}");
            assert_eq!(key(&toml), format!("workspaces.{limit}"));
            assert_eq!(
                key(&format!("mirrors = \"/m\"\n{limit} = 0")),
                format!("workspaces.{limit}")
            );
        }
        assert!(toml::from_str::<File>("mirrors = \"/m\"\nextra = 1").is_err());
    }

    #[test]
    fn the_contexts_file_round_trips_sorted_and_refuses_what_does_not_check() {
        let dir = std::env::temp_dir().join(format!("meada-contexts-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let entries =
            meada_core::named::version_entries("t", "[[repository]]\nrepo = \"h/o/y\"\n", 5)
                .unwrap();
        let version = meada_core::named::version_id(&entries);
        let context = |id: &str| Context {
            id: id.into(),
            version: version.clone(),
            operators: vec!["alice".into()],
            entries: entries.clone(),
            excluded: Vec::new(),
        };
        let digest = write_contexts(
            &dir,
            &[context("bbbbbbbbbbbbbbbb"), context("aaaaaaaaaaaaaaaa")],
        )
        .unwrap();
        let (read, again) = read_contexts(&dir).unwrap().unwrap();
        assert_eq!(digest, again);
        let ids: Vec<&str> = read.iter().map(|c| c.id.as_str()).collect();
        assert_eq!(ids, ["aaaaaaaaaaaaaaaa", "bbbbbbbbbbbbbbbb"]);
        assert_eq!(read[0].identities(), ["h/o/y"]);
        // A version id that is not the entries' is refused.
        let mut forged = context("cccccccccccccccc");
        forged.version = "0000000000000000".into();
        std::fs::write(dir.join(CONTEXTS), contexts_text(&[forged])).unwrap();
        assert!(read_contexts(&dir).is_err());
        std::fs::write(dir.join(CONTEXTS), "{\"contexts\": 3}").unwrap();
        assert!(read_contexts(&dir).is_err());
        // A file past the bound is refused unread, and so is an operator that is no login.
        let mut forged = context("dddddddddddddddd");
        forged.operators = vec!["not a login".into()];
        std::fs::write(dir.join(CONTEXTS), contexts_text(&[forged])).unwrap();
        assert!(read_contexts(&dir).is_err());
        let big = std::fs::File::create(dir.join(CONTEXTS)).unwrap();
        big.set_len(CONTEXTS_MAX + 1).unwrap();
        assert!(read_contexts(&dir).is_err());
        std::fs::remove_file(dir.join(CONTEXTS)).unwrap();
        assert!(read_contexts(&dir).unwrap().is_none());
    }

    /// The membership keeps, beside a context's entries the sponsor rule left in, the identities
    /// it excluded, sorted (#510): what the generation records for the node's admission. A
    /// context left with no entry is dropped.
    #[test]
    fn the_membership_records_each_context_s_exclusions() {
        let dir = std::env::temp_dir().join(format!("meada-membership-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let text = "[[repository]]\nrepo = \"h/o/y\"\n[[repository]]\nrepo = \"h/o/b\"\n\
                    [[repository]]\nrepo = \"h/o/a\"\n";
        let entries = meada_core::named::version_entries("t", text, 5).unwrap();
        let context = |id: &str| Context {
            id: id.into(),
            version: meada_core::named::version_id(&entries),
            operators: vec!["alice".into()],
            entries: entries.clone(),
            excluded: Vec::new(),
        };
        let (kept, plain, gone) = ("aaaaaaaaaaaaaaaa", "bbbbbbbbbbbbbbbb", "cccccccccccccccc");
        let digest = write_contexts(&dir, &[context(kept), context(plain), context(gone)]).unwrap();
        let version = crate::snapshot::versions::NO_VERSION;
        let mut out = std::collections::BTreeSet::new();
        for identity in ["h/o/a", "h/o/b"] {
            out.insert((kept.to_string(), identity.to_string()));
        }
        for identity in ["h/o/a", "h/o/b", "h/o/y"] {
            out.insert((gone.to_string(), identity.to_string()));
        }
        let none = std::collections::BTreeSet::new();
        crate::snapshot::versions::write_excluded_with(&dir, version, &none, Some((&digest, &out)))
            .unwrap();
        let found = membership(&dir, "", &[]).unwrap();
        let ids: Vec<&str> = found.contexts.iter().map(|c| c.id.as_str()).collect();
        assert_eq!(ids, [kept, plain]);
        assert_eq!(found.contexts[0].identities(), ["h/o/y"]);
        assert_eq!(found.contexts[0].excluded, ["h/o/a", "h/o/b"]);
        assert!(found.contexts[1].excluded.is_empty());
        assert_eq!(found.members, ["h/o/a", "h/o/b", "h/o/y"]);
    }
}
