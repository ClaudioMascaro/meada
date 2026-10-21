//! Serving a pulled generation (ADR 0012, feature `snapshot`): what `meada mcp --listen
//! --workspace <manifest> --generations` and `meada status --generations` read from a node's
//! `local_dir`. A node's manifest contributes only its module sections; the repositories are
//! the ones the current generation recorded (`<local_dir>/current`, the layout `pull` writes).
//! Everything here only reads: a call reads the pointer file and the generation document, and
//! the node's `meada snapshot pull` is what moves the pointer.

use std::fs;
use std::path::{Path, PathBuf};
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
    /// Its repositories, in the document's order, each mirror and index inside the generation's
    /// directory.
    pub listed: Manifest,
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
    let shown = local.display().to_string();
    let Some((id, _)) = pull::current(local)? else {
        return Err(Msg::SnapshotNoneHeld(&shown).text().into());
    };
    let dir = local.join(&id);
    let file = dir.join(pull::DOCUMENT);
    let not = || Msg::SnapshotNotDocument(&file.display().to_string()).text();
    let bytes = config::read_bounded(&file, DOCUMENT_MAX)?;
    let document: Document = serde_json::from_slice(&bytes).map_err(|_| not())?;
    if document.generation != id {
        return Err(not().into());
    }
    let listed = Manifest::of_generation(&dir, &document)?;
    Ok(Held { id, listed })
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
