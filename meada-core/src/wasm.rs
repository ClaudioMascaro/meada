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
    crate::manifest::validate(file, src)
        .err()
        .unwrap_or_default()
}

/// The workspace version id of the workspace text `src`; `undefined` when it is not valid.
#[wasm_bindgen(js_name = workspaceVersionId)]
pub fn workspace_version_id(src: &str) -> Option<String> {
    let entries = crate::manifest::validate("", src).ok()?;
    let blocks = crate::ids::entry_blocks(src);
    let entries: Vec<_> = (entries.iter().zip(&blocks))
        .map(|(e, blocks)| (e.repo.as_str(), e.path.as_str(), blocks))
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
