//! A manifest error never quotes a value the file wrote (#449, #545): a secret pasted under the
//! wrong key must not reach a log or a viewer. One marker goes into every case `describe`
//! handles past the syntax check, and no message, in either language, may hold it.
//!
//! This binary holds one test, which sets the process-wide language: no other test shares it.

use meada_core::manifest::{parse, validate};
use serde::Deserialize;

const MARKER: &str = "MARKER-s3cr3t";
const FILE: &str = "ops/meada.workspace.toml";

/// A schema of a shape the manifests do not have: a `char`, which makes serde's `invalid value`.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
#[allow(dead_code)]
struct Probe {
    letter: Option<char>,
}

/// A whole file that its type refuses, which no key spans.
#[derive(Deserialize)]
#[serde(try_from = "toml::Table")]
struct Whole;

impl TryFrom<toml::Table> for Whole {
    type Error = String;

    fn try_from(table: toml::Table) -> Result<Self, String> {
        Err(format!(
            "invalid value: string \"{table}\", expected nothing"
        ))
    }
}

/// Every message the manifest schema gives for a marker written in each case.
fn messages() -> Vec<(&'static str, String)> {
    let entry = |body: &str| format!("[[repository]]\nrepo = \"a.com/x\"\npath = \"x\"\n{body}");
    let mut all: Vec<(&str, String)> = Vec::new();
    let mut add = |case: &'static str, src: String| {
        let problems = validate(FILE, &src).err().unwrap_or_default();
        assert!(!problems.is_empty(), "{case}: no error for {src:?}");
        all.extend(problems.into_iter().map(|p| (case, p)));
    };
    add("unknown field", entry(&format!("branch = \"{MARKER}\"\n")));
    add(
        "missing field",
        format!("[[repository]]\npath = \"{MARKER}\"\n"),
    );
    add("invalid type", entry(&format!("serve = \"{MARKER}\"\n")));
    add(
        "unknown variant",
        entry(&format!(
            "[[repository.resource]]\nkind = \"pg\"\nid = \"a\"\nrole = \"{MARKER}\"\n"
        )),
    );
    add(
        "unknown variant with a backtick",
        entry(&format!(
            "[[repository.resource]]\nkind = \"pg\"\nid = \"a\"\nrole = \"x`, expected `{MARKER}\"\n"
        )),
    );
    add(
        "no leading slash, prefix",
        entry(&format!("[repository.serve]\nprefix = \"{MARKER}\"\n")),
    );
    add(
        "no leading slash, strip",
        entry(&format!("[repository.serve]\nstrip = \"{MARKER}\"\n")),
    );
    add(
        "path outside, absolute",
        entry(&format!(
            "[repository.serve]\ndocuments = [\"/{MARKER}\"]\n"
        )),
    );
    add(
        "path outside, parent",
        entry(&format!(
            "[repository.serve]\ndocuments = [\"{MARKER}/../..\"]\n"
        )),
    );
    add(
        "not lowercase",
        entry(&format!(
            "[[repository.resource]]\nkind = \"{MARKER}\"\nid = \"a\"\nrole = \"read\"\n"
        )),
    );
    let mut refused = |case: &'static str, error: String| all.push((case, error));
    let letter = format!("letter = \"{MARKER}\"\n");
    refused(
        "invalid value",
        parse::<Probe>(FILE, &letter, "probe").err().unwrap(),
    );
    // A comment first, so the error found at the start of the file lies in no key.
    let whole = format!("# one\nkey = \"{MARKER}\"\n");
    refused(
        "no key found",
        parse::<Whole>(FILE, &whole, "probe").err().unwrap(),
    );
    all
}

#[test]
fn no_manifest_error_quotes_a_written_value_in_either_language() {
    for lang in [None, Some("pt_BR.UTF-8")] {
        meada_core::msg::init(lang);
        for (case, message) in messages() {
            assert!(
                !message.contains(MARKER),
                "{case} ({lang:?}) quotes the value: {message}"
            );
            // The no-key case is Meada's own text, not serde's.
            if case == "no key found" {
                let own = ["a value is not valid here", "um valor não é válido aqui"];
                assert!(own.iter().any(|t| message.ends_with(t)), "{message}");
            }
        }
    }
    meada_core::msg::init(None);
}
