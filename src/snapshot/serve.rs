//! Serving a pulled generation (ADR 0012, feature `snapshot`): what `meada mcp --listen
//! --workspace <manifest> --generations` and `meada status --generations` read from a node's
//! `local_dir`. A node's manifest contributes only its module sections; the repositories are
//! the ones the current generation recorded (`<local_dir>/current`, the layout `pull` writes).
//! Everything here only reads: a call reads the pointer file and the generation document, and
//! the node's `meada snapshot pull` is what moves the pointer.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use super::store::Store;
use super::{CURRENT_MAX, Config, DOCUMENT_MAX, Document, is_id, pull};
use crate::msg::Msg;
use crate::workspace::Manifest;
use crate::{Result, config};

/// How long the status probe waits for the store before it calls it unreachable.
const REACH_TIMEOUT: Duration = Duration::from_secs(2);

/// A generation a node holds, as its document records it.
pub struct Held {
    pub id: String,
    /// `<local_dir>/<id>`.
    pub dir: PathBuf,
    /// Its repositories, in the document's order, each mirror and index inside the generation's
    /// directory.
    pub listed: Manifest,
}

/// The generation `<local_dir>/current` names, read once per generation: a generation's
/// directory never changes once the pull renamed it into place, so a call reads only the
/// pointer file, and its document and the mirrors' configuration only when the pointer names
/// another generation. Only the last one read is kept; a call still answering from an earlier
/// one holds its own `Arc`.
pub struct Current {
    local: PathBuf,
    last: Mutex<Option<Arc<Held>>>,
}

impl Current {
    pub fn new(local: PathBuf) -> Self {
        Self {
            local,
            last: Mutex::new(None),
        }
    }

    /// The generation the pointer file names now, as `held` reads it.
    pub fn get(&self) -> Result<Arc<Held>> {
        let id = pointer(&self.local)?;
        let mut last = self.last.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(held) = last.as_ref().filter(|held| held.id == id) {
            return Ok(held.clone());
        }
        let held = Arc::new(load(&self.local, id)?);
        *last = Some(held.clone());
        Ok(held)
    }
}

/// The `[snapshot]` section of the manifest at `manifest` and its `local_dir`: the only part of
/// a node's manifest that serving reads.
pub fn node(manifest: &Path) -> Result<(Config, PathBuf)> {
    let shown = manifest.display().to_string();
    let listed = Manifest::read(manifest)?;
    let config = (listed.snapshot).ok_or_else(|| Msg::SnapshotNoSection(&shown).text())?;
    let local = (config.local_dir.clone()).ok_or_else(|| Msg::SnapshotNoLocalDir(&shown).text())?;
    Ok((config, local))
}

/// The generation `<local_dir>/current` names, its document read and checked as the pull
/// stored it. Without a pointer the node holds nothing to serve: the error names the pull.
pub fn held(local: &Path) -> Result<Held> {
    load(local, pointer(local)?)
}

/// The id `<local_dir>/current` names; without one, an error naming the pull.
fn pointer(local: &Path) -> Result<String> {
    match pull::current(local)? {
        Some((id, _)) => Ok(id),
        None => Err(Msg::SnapshotNoneHeld(&local.display().to_string())
            .text()
            .into()),
    }
}

/// The generation `id` of `local`, its document read and checked as the pull stored it.
fn load(local: &Path, id: String) -> Result<Held> {
    let dir = local.join(&id);
    let file = dir.join(pull::DOCUMENT);
    let not = || Msg::SnapshotNotDocument(&file.display().to_string()).text();
    let bytes = config::read_bounded(&file, DOCUMENT_MAX)?;
    let document: Document = serde_json::from_slice(&bytes).map_err(|_| not())?;
    if document.generation != id {
        return Err(not().into());
    }
    let listed = Manifest::of_generation(&dir, &document)?;
    Ok(Held { id, dir, listed })
}

/// The ids of the generations `local` holds, sorted: its directories named by an id. A pull in
/// progress is `<id>.partial`, held by no one.
pub fn ids(local: &Path) -> Vec<String> {
    let mut ids: Vec<String> = fs::read_dir(local)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|name| is_id(name))
        .collect();
    ids.sort();
    ids
}

/// The generation id the store's `current` holds, or why the store did not say within
/// `REACH_TIMEOUT`.
pub fn reach(config: &Config) -> std::result::Result<String, String> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| e.to_string())?;
    let ask = async {
        let store = Store::connect(config)?;
        let key = config.key("current");
        let bytes = store.get(&key, CURRENT_MAX).await?;
        pull::store_pointer(
            &key,
            &bytes.ok_or_else(|| Msg::SnapshotNoCurrent(&key).text())?,
        )
    };
    let answer = runtime.block_on(async { tokio::time::timeout(REACH_TIMEOUT, ask).await });
    // Dropping the runtime would wait for a blocking thread still in a DNS lookup.
    runtime.shutdown_background();
    answer.unwrap_or_else(|_| Err(Msg::SnapshotTimeout(REACH_TIMEOUT.as_millis() as u64).text()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::Engine;

    const REPO: &str = "example.com/serve/app";
    const BUNDLE: &str = "00000000000000b1";
    const A: &str = "000000000000000a";
    const B: &str = "000000000000000b";

    fn scratch(case: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("meada-serve-{case}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// The document of generation `id` as a pull stores it: one repository, `REPO`.
    fn document(id: &str) -> serde_json::Value {
        serde_json::json!({"generation": id, "repositories": [{
            "identity": REPO, "head": "0".repeat(40),
            "bundle": format!("prod/repos/{REPO}/{BUNDLE}.tar"), "blocks": {},
            "size": 0, "blake3": "0".repeat(64)}]})
    }

    /// Generation `id` of `local` made by hand, as a pull leaves it: `document`, with `REPO`'s
    /// mirror holding `files` and its index built over them. Returns the mirror.
    fn generation(local: &Path, id: &str, files: &[(&str, &str)]) -> PathBuf {
        let bundle = local.join(id).join(BUNDLE);
        let mirror = bundle.join("mirror");
        fs::create_dir_all(&mirror).unwrap();
        for (path, text) in files {
            fs::write(mirror.join(path), text).unwrap();
        }
        let serve = crate::config::Serve::default();
        crate::index::open(&bundle.join("index.db"), &mirror, REPO, &serve).unwrap();
        write(local, id, &document(id));
        mirror
    }

    fn write(local: &Path, id: &str, document: &serde_json::Value) {
        let bytes = serde_json::to_vec(document).unwrap();
        fs::write(local.join(id).join(pull::DOCUMENT), bytes).unwrap();
    }

    fn point(local: &Path, id: &str) {
        fs::write(local.join(pull::POINTER), format!("{id} 0\n")).unwrap();
    }

    /// Why `held` refuses what `local` holds.
    fn refused(local: &Path) -> String {
        match held(local) {
            Ok(_) => panic!("{} was served", local.display()),
            Err(e) => e.to_string(),
        }
    }

    #[test]
    fn a_call_answers_from_the_generation_it_was_routed_by() {
        let local = scratch("routed");
        generation(&local, A, &[("a.ts", "export function alpha() {}\n")]);
        generation(&local, B, &[("a.ts", "export function beta() {}\n")]);
        point(&local, A);
        let current = Current::new(local.clone());
        let routed = current.get().unwrap();
        // The pointer moves between the route and the answer.
        point(&local, B);
        let engine = Engine::open_held(&routed, REPO).unwrap();
        assert!(engine.search("alpha", None, None).unwrap().contains("a.ts"));
        let beta = engine.search("beta", None, None).unwrap();
        assert!(beta.starts_with("no matches"), "{beta}");
        assert_eq!(current.get().unwrap().id, B);
        // The cache let go of the generation no longer current: only the call holds it.
        assert_eq!(Arc::strong_count(&routed), 1);
    }

    #[test]
    fn a_generation_is_read_once_and_then_only_its_pointer() {
        let local = scratch("once");
        generation(&local, A, &[("a.ts", "export function alpha() {}\n")]);
        point(&local, A);
        let current = Current::new(local.clone());
        let first = current.get().unwrap();
        fs::remove_file(local.join(A).join(pull::DOCUMENT)).unwrap();
        assert!(Arc::ptr_eq(&first, &current.get().unwrap()));
        fs::remove_file(local.join(pull::POINTER)).unwrap();
        let e = current.get().err().unwrap().to_string();
        assert!(e.contains("meada snapshot pull"), "{e}");
    }

    /// A held generation is as hostile as the store it came from: a document that does not
    /// hold what the pull checked is refused before any path is built from it.
    #[test]
    fn a_held_document_the_pull_would_refuse_is_not_served() {
        let local = scratch("hostile");
        generation(&local, A, &[("a.ts", "export function alpha() {}\n")]);
        point(&local, A);
        assert!(held(&local).is_ok());
        let file = local.join(A).join(pull::DOCUMENT).display().to_string();
        let not = format!("{file}: not the generation document of its id");
        let entry = |edit: &dyn Fn(&mut serde_json::Value)| {
            let mut document = document(A);
            edit(&mut document["repositories"][0]);
            document
        };
        let cases = [
            (
                "a bundle id that is not an id",
                entry(&|e| e["bundle"] = format!("prod/repos/{REPO}/../../x.tar").into()),
                not.clone(),
            ),
            (
                "blocks that are not an object",
                entry(&|e| e["blocks"] = serde_json::json!(["serve"])),
                not.clone(),
            ),
            (
                "another generation's document",
                serde_json::json!({"generation": B, "repositories": document(A)["repositories"]}),
                not.clone(),
            ),
            (
                "an invalid identity",
                entry(&|e| e["identity"] = "example.com/../escaped".into()),
                format!("{file}: example.com/../escaped: "),
            ),
        ];
        for (case, document, expected) in cases {
            write(&local, A, &document);
            let e = refused(&local);
            assert!(e.contains(&expected), "{case}: {e}");
        }
    }

    /// A pull of the generation already current changes nothing: the fix of a missing index
    /// removes the generation's directory first, or publishes another one.
    #[test]
    fn a_missing_index_names_the_generation_directory_to_remove() {
        let local = scratch("missing");
        generation(&local, A, &[("a.ts", "export function alpha() {}\n")]);
        point(&local, A);
        fs::remove_file(local.join(A).join(BUNDLE).join("index.db")).unwrap();
        let held = held(&local).unwrap();
        let e = Engine::open_held(&held, REPO).err().unwrap().to_string();
        let dir = local.join(A).display().to_string();
        assert!(e.contains(&format!("generation {A}; remove {dir} and run meada snapshot pull again, or publish a new generation")), "{e}");
    }

    #[test]
    fn an_oversized_mirror_config_fails_the_generation_naming_the_file() {
        for name in ["meada.toml", "meada.decisions.toml"] {
            let local = scratch(&format!("large-{name}"));
            let mirror = generation(&local, A, &[("a.ts", "export function alpha() {}\n")]);
            point(&local, A);
            let file = mirror.join(name);
            fs::write(
                &file,
                " ".repeat(config::GENERATION_CONFIG_MAX as usize + 1),
            )
            .unwrap();
            let e = refused(&local);
            assert!(e.contains(&file.display().to_string()), "{e}");
            assert!(e.contains("larger than"), "{e}");
            fs::write(&file, " ".repeat(config::GENERATION_CONFIG_MAX as usize)).unwrap();
            assert!(held(&local).is_ok(), "{name}");
        }
    }
}
