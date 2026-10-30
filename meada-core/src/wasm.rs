//! The browser's bindings (feature `wasm-bindgen`): the console's workspace editor validates and
//! names a version with the same functions as the CLI and marks what a subject may not change
//! with the server's authorization, and the graph views read an export with the types the binary
//! writes it with.

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
        .map(|(e, blocks)| {
            (
                e.repo.as_str(),
                e.path.as_str(),
                blocks,
                e.owner.as_deref(),
                e.added_by.as_deref(),
            )
        })
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

/// The workspace text `src` as the JSON entries of a version document, the form
/// `authorizeEntries` and `composeEntries` take; throws import's problems when it is not valid.
#[wasm_bindgen(js_name = entriesJson)]
pub fn entries_json(src: &str) -> Result<String, String> {
    let entries = crate::workspace::version_entries("entries", src).map_err(|e| e.join("\n"))?;
    serde_json::to_string(&entries).map_err(|error| error.to_string())
}

/// The entries `role` (`operator`, `team_operator` or `viewer`) may not change from `current` to
/// `entries`, as JSON `[{identity, change, reason}]`, none when it may. `login` and `github_host`
/// are the subject's GitHub login and the GitHub grant provider's host, `undefined` (or `null`)
/// without it. Every other argument is JSON: `groups` an array of names, `grant` `"*"` or an
/// array of identities, the entries a version's `repositories`.
#[wasm_bindgen(js_name = authorizeEntries)]
pub fn authorize_entries(
    role: &str,
    groups: &str,
    grant: &str,
    login: Option<String>,
    github_host: Option<String>,
    current: &str,
    entries: &str,
) -> Result<String, String> {
    let role = serde_json::Value::from(role);
    let refused = crate::authorize::authorize_entries(
        serde_json::from_value(role).map_err(|e| e.to_string())?,
        &from_json::<Vec<String>>(groups)?,
        &from_json(grant)?,
        login.as_deref(),
        github_host.as_deref(),
        &from_json::<Vec<_>>(current)?,
        &from_json::<Vec<_>>(entries)?,
    );
    serde_json::to_string(&refused).map_err(|e| e.to_string())
}

/// The entries a team operator's save stores (`compose_entries`), its additions attributed, as
/// JSON; the arguments as `authorizeEntries` takes them.
#[wasm_bindgen(js_name = composeEntries)]
pub fn compose_entries(
    grant: &str,
    login: Option<String>,
    github_host: Option<String>,
    current: &str,
    submitted: &str,
) -> Result<String, String> {
    let composed = crate::authorize::compose_entries(
        &from_json(grant)?,
        login.as_deref(),
        github_host.as_deref(),
        &from_json::<Vec<_>>(current)?,
        &from_json::<Vec<_>>(submitted)?,
    );
    serde_json::to_string(&composed).map_err(|e| e.to_string())
}

fn from_json<T: serde::de::DeserializeOwned>(text: &str) -> Result<T, String> {
    serde_json::from_str(text).map_err(|e| e.to_string())
}

/// The entries of the named workspace text `src` (ADR 0021) as the JSON a version document
/// holds them, with no `path`, `owner` or `added_by`; throws the problems when it is not valid.
/// `max` is `[workspaces] max_entries`.
#[wasm_bindgen(js_name = namedEntriesJson)]
pub fn named_entries_json(src: &str, max: usize) -> Result<String, String> {
    let entries = crate::named::version_entries("entries", src, max).map_err(|e| e.join("\n"))?;
    serde_json::to_string(&entries).map_err(|error| error.to_string())
}

/// The version id of the named workspace text `src`; `undefined` when it is not valid.
#[wasm_bindgen(js_name = namedVersionId)]
pub fn named_version_id(src: &str, max: usize) -> Option<String> {
    let entries = crate::named::version_entries("entries", src, max).ok()?;
    Some(crate::named::version_id(&entries))
}

/// A named workspace text from the JSON entries of a version document, in the form
/// `namedEntriesJson` reads back.
#[wasm_bindgen(js_name = namedEntriesText)]
pub fn named_entries_text(entries: &str) -> Result<String, String> {
    let entries: Vec<serde_json::Value> = from_json(entries)?;
    crate::workspace::entries_text(&entries).ok_or_else(|| "not repository entries".to_string())
}

/// Whether a selection may be made (ADR 0020): JSON `{allowed, named, hidden}`. `eligible` is
/// the array of identities the login holds an accepted role on (the session's `github_grant`),
/// `grant` `"*"` or an array of identities the session reads.
#[wasm_bindgen(js_name = namedAuthorize)]
pub fn named_authorize(selected: &str, eligible: &str, grant: &str) -> Result<String, String> {
    let creation = crate::named::creation(
        &from_json::<Vec<String>>(selected)?,
        &from_json(eligible)?,
        &from_json(grant)?,
    );
    serde_json::to_string(&creation).map_err(|e| e.to_string())
}

/// Whether a caller sees a workspace of `entries` (a JSON array of identities): its operators
/// always, anyone else only when `grant` reads every entry.
#[wasm_bindgen(js_name = namedVisible)]
pub fn named_visible(operates: bool, entries: &str, grant: &str) -> Result<bool, String> {
    Ok(crate::named::visible(
        operates,
        &from_json::<Vec<String>>(entries)?,
        &from_json(grant)?,
    ))
}

/// The effective configuration of the named entries (ADR 0021, contract 6) as JSON
/// `[{repo, source, blocks, ignored, coverage, diagnostics}]`. `entries` is the JSON
/// `namedEntriesJson` gives, `facts` an object by identity of `{held, own, documents,
/// excluded}` the console reads from the generation it holds; an identity without facts is a
/// repository no held generation has. The decision is the console's: the browser calls it so the
/// guided form and the server agree.
#[wasm_bindgen(js_name = namedEffective)]
pub fn named_effective(entries: &str, facts: &str) -> Result<String, String> {
    let found = crate::effective::effective(
        &from_json::<Vec<serde_json::Value>>(entries)?,
        &from_json(facts)?,
    );
    serde_json::to_string(&found).map_err(|e| e.to_string())
}
