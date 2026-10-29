//! The browser's bindings (feature `wasm-bindgen`): the console's workspace editor validates and
//! names a version with the same functions as the CLI, and the graph views read an export with
//! the types the binary writes it with.

use wasm_bindgen::prelude::*;

/// Picks the language of the messages, as the CLI does from `LANG`: pt-BR when `lang` starts
/// with `pt`, English otherwise.
#[wasm_bindgen]
pub fn init(lang: &str) {
    crate::msg::init(Some(lang));
}

/// The problems of the workspace text `src`, shown as `file`, as the CLI prints them; none when
/// it is valid.
#[wasm_bindgen]
pub fn validate(file: &str, src: &str) -> Vec<String> {
    crate::manifest::validate_import(file, src)
        .err()
        .unwrap_or_default()
}

/// The workspace version id of the workspace text `src`; `undefined` when it is not valid.
#[wasm_bindgen(js_name = workspaceVersionId)]
pub fn workspace_version_id(src: &str) -> Option<String> {
    let entries = crate::manifest::validate_import("entries", src).ok()?;
    let blocks = crate::ids::entry_blocks(src);
    let entries: Vec<_> = (entries.iter().zip(&blocks))
        .map(|(e, blocks)| (e.repo.as_str(), e.path.as_str(), blocks, e.owner.as_deref()))
        .collect();
    Some(crate::ids::workspace_version_id(&entries))
}

/// The export `text` (what `meada export` prints) read into the export types and written again,
/// so the views get the canonical JSON to `JSON.parse`; throws, naming the field, when it does
/// not fit them.
#[wasm_bindgen(js_name = parseExport)]
pub fn parse_export(text: &str) -> Result<String, String> {
    let export = crate::export::parse(text)?;
    export.write().map_err(|e| e.to_string())
}

/// Entry changes and their dotted keys, computed locally with import's validation.
#[wasm_bindgen(js_name = workspaceDiff)]
pub fn workspace_diff(before: &str, after: &str) -> Result<String, String> {
    let diff =
        crate::workspace::diff("entries", before, after).map_err(|errors| errors.join("\n"))?;
    serde_json::to_string(&diff).map_err(|error| error.to_string())
}

/// A checked version's JSON repositories in the same TOML form as `workspace show`.
#[wasm_bindgen(js_name = entriesText)]
pub fn entries_text(repositories: &str) -> Result<String, String> {
    let repositories: Vec<serde_json::Value> =
        serde_json::from_str(repositories).map_err(|error| error.to_string())?;
    crate::workspace::entries_text(&repositories)
        .ok_or_else(|| "not repository entries".to_string())
}
