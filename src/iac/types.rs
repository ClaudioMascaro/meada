//! The type table (ADR 0025, ADR 0026, #656): what an IaC type provisions, read from
//! `types.toml`, which the binary embeds. An entry names the contract kind (a channel's broker or
//! a resource's kind), the argument that holds the physical name and the forms an ARN or a URL
//! holding the name takes. The file's own header documents its format for later entries.

use std::sync::LazyLock;

use serde::Deserialize;

/// The table as the repository versions it.
const TABLE: &str = include_str!("types.toml");

/// The longest text `Entry::read` matches against its forms.
// ponytail: an ARN or a URL is well under a few hundred characters; a longer text is the name as
// it is, which keeps the matching (quadratic in the text) bounded.
const FORM_TEXT_MAX: usize = 1024;

/// The contract kind an entry provisions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Contract {
    Channel,
    Resource,
}

/// One type of the table.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Entry {
    pub format: String,
    #[serde(rename = "type")]
    pub ty: String,
    pub contract: Contract,
    pub kind: String,
    /// The argument that holds the physical name.
    pub name: String,
    #[serde(default)]
    pub forms: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Table {
    entry: Vec<Entry>,
    #[serde(default)]
    workload: Vec<Workload>,
}

/// A type that runs code, with the image and code paths its format reads.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Workload {
    pub format: String,
    #[serde(rename = "type")]
    pub ty: String,
    pub images: Vec<String>,
    pub code: Option<String>,
}

static WORKLOADS: LazyLock<Vec<Workload>> =
    LazyLock::new(|| toml::from_str::<Table>(TABLE).map_or_else(|_| Vec::new(), |t| t.workload));

pub(crate) fn workload(format: &str, ty: &str) -> Option<&'static Workload> {
    WORKLOADS.iter().find(|w| w.format == format && w.ty == ty)
}

static ENTRIES: LazyLock<Vec<Entry>> = LazyLock::new(|| {
    // The table is the repository's own file, checked by `the_table_parses`.
    toml::from_str::<Table>(TABLE).map_or_else(|_| Vec::new(), |t| t.entry)
});

/// The entry of `ty` in `format` (`terraform`), when the table lists it.
pub(crate) fn entry(format: &str, ty: &str) -> Option<&'static Entry> {
    ENTRIES.iter().find(|e| e.format == format && e.ty == ty)
}

/// The format of the IaC file at `path` as the table names it: `terraform` for a `.tf` or
/// `.tofu` file or its JSON equivalent, `None` for a format no entry has yet.
pub(crate) fn format_of(path: &str) -> Option<&'static str> {
    (path.ends_with(".tf") || path.ends_with(".tofu") || super::json::is_file(path))
        .then_some("terraform")
}

/// The unique contract named by an AWS structured identifier. Forms stay in the type table.
pub(crate) fn identified(text: &str) -> Option<String> {
    if !(text.starts_with("arn:") || text.starts_with("https://") || text.starts_with("s3://")) {
        return None;
    }
    let service = text
        .strip_prefix("arn:")
        .and_then(|_| text.split(':').nth(2));
    let mut contracts = ENTRIES
        .iter()
        // An ARN's service is explicit. Do not run unrelated service forms for every scalar.
        .filter(|entry| {
            service.is_none_or(|service| {
                entry
                    .forms
                    .iter()
                    .any(|form| form.starts_with("arn:") && form.split(':').nth(2) == Some(service))
            })
        })
        .filter_map(|entry| {
            let name = entry.read(text);
            (name != text).then(|| entry.contract(name))
        })
        .collect::<std::collections::BTreeSet<_>>();
    (contracts.len() == 1).then(|| contracts.pop_first().unwrap())
}

impl Entry {
    /// The contract a node of this type named `name` provisions.
    pub(crate) fn contract(&self, name: &str) -> String {
        let prefix = match self.contract {
            Contract::Channel => "channel",
            Contract::Resource => "resource",
        };
        format!("{prefix}::{}::{name}", self.kind)
    }

    /// The name `text` holds: what `{name}` matches in the first form `text` fits, else `text`
    /// itself.
    pub(crate) fn read<'t>(&self, text: &'t str) -> &'t str {
        if text.len() > FORM_TEXT_MAX {
            return text;
        }
        (self.forms.iter())
            .find_map(|form| fits(form, text))
            .unwrap_or(text)
    }
}

/// One piece of a form: literal text, `*` or `{name}`.
#[derive(Clone, Copy)]
enum Piece<'f> {
    Text(&'f str),
    Any,
    Name(bool),
    Suffix,
}

fn pieces(form: &str) -> Vec<Piece<'_>> {
    let mut out = Vec::new();
    let mut rest = form;
    while !rest.is_empty() {
        let star = rest.find('*');
        let name = rest.find("{name}").or_else(|| rest.find("{path}"));
        let suffix = rest.find('?');
        let next = [star, name, suffix].into_iter().flatten().min();
        match next {
            Some(0) if star == Some(0) => {
                out.push(Piece::Any);
                rest = &rest[1..];
            }
            Some(0) if name == Some(0) => {
                out.push(Piece::Name(rest.starts_with("{path}")));
                rest = &rest["{name}".len()..];
            }
            Some(0) => {
                out.push(Piece::Suffix);
                rest = &rest[1..];
            }
            Some(at) => {
                out.push(Piece::Text(&rest[..at]));
                rest = &rest[at..];
            }
            None => {
                out.push(Piece::Text(rest));
                rest = "";
            }
        }
    }
    out
}

/// What `{name}` matches when `text` fits `form`: the form whole, or the form then `/` and
/// anything.
fn fits<'t>(form: &str, text: &'t str) -> Option<&'t str> {
    let pieces = pieces(form);
    let mut failed = std::collections::HashSet::new();
    let mut name = None;
    match matches(&pieces, text, 0, 0, &mut failed, &mut name) {
        true => name,
        false => None,
    }
}

/// Whether `text` from `at` fits `pieces` from `p`, setting `name` on the way; `failed` holds the
/// positions already known not to fit, so the walk is quadratic at worst.
fn matches<'t>(
    pieces: &[Piece],
    text: &'t str,
    p: usize,
    at: usize,
    failed: &mut std::collections::HashSet<(usize, usize)>,
    name: &mut Option<&'t str>,
) -> bool {
    if failed.contains(&(p, at)) {
        return false;
    }
    let found = match pieces.get(p) {
        None => {
            at == text.len()
                || (!pieces.iter().any(|p| matches!(p, Piece::Name(true)))
                    && text[at..].starts_with('/'))
        }
        Some(Piece::Text(t)) => {
            text[at..].starts_with(t) && matches(pieces, text, p + 1, at + t.len(), failed, name)
        }
        Some(Piece::Suffix) => {
            text.as_bytes()
                .get(at)
                .is_some_and(u8::is_ascii_alphanumeric)
                && matches(pieces, text, p + 1, at + 1, failed, name)
        }
        Some(piece @ (Piece::Any | Piece::Name(_))) => {
            let run = text[at..]
                .find(|c| c == ':' || (c == '/' && !matches!(piece, Piece::Name(true))))
                .map_or(text.len(), |i| at + i);
            let ends = (at + 1..=run).filter(|&e| text.is_char_boundary(e));
            let mut hit = false;
            for end in ends {
                if matches(pieces, text, p + 1, end, failed, name) {
                    if matches!(piece, Piece::Name(_)) {
                        *name = Some(&text[at..end]);
                    }
                    hit = true;
                    break;
                }
            }
            hit
        }
    };
    if !found {
        failed.insert((p, at));
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_table_parses() {
        let table: Table = toml::from_str(TABLE).unwrap();
        assert!(!table.entry.is_empty());
        assert_eq!(table.entry.len(), ENTRIES.len());
    }

    /// Every entry has a fixture case: a block of its type in the provisioning fixture.
    #[test]
    fn every_entry_has_a_fixture_case() {
        let root = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/provision-infra"
        );
        let mut text = String::new();
        let mut dirs = vec![
            std::path::PathBuf::from(root),
            std::path::PathBuf::from(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/fixtures/aws-infra"
            )),
        ];
        while let Some(dir) = dirs.pop() {
            for e in std::fs::read_dir(&dir).unwrap() {
                let path = e.unwrap().path();
                if path.is_dir() {
                    dirs.push(path);
                } else if format_of(&path.to_string_lossy()).is_some() {
                    text += &std::fs::read_to_string(&path).unwrap();
                }
            }
        }
        for e in ENTRIES.iter() {
            assert!(
                text.contains(&format!("resource \"{}\"", e.ty)),
                "no fixture case for {}",
                e.ty
            );
        }
    }

    #[test]
    fn the_entries_name_their_contract_and_argument() {
        let sqs = entry("terraform", "aws_sqs_queue").unwrap();
        assert_eq!(sqs.contract("orders"), "channel::sqs::orders");
        assert_eq!(sqs.name, "name");
        let s3 = entry("terraform", "aws_s3_bucket").unwrap();
        assert_eq!(s3.contract("archive"), "resource::s3::archive");
        assert_eq!(s3.name, "bucket");
        assert!(entry("terraform", "aws_unknown_resource").is_none());
        assert!(entry("cloudformation", "aws_sqs_queue").is_none());
    }

    #[test]
    fn a_name_is_read_out_of_an_arn_or_a_url() {
        let sqs = entry("terraform", "aws_sqs_queue").unwrap();
        assert_eq!(
            sqs.read("arn:aws:sqs:eu-west-1:123456789012:orders"),
            "orders"
        );
        let url = "https://sqs.eu-west-1.amazonaws.com/123456789012/orders.fifo";
        assert_eq!(sqs.read(url), "orders.fifo");
        let legacy = "https://eu-west-1.queue.amazonaws.com/123456789012/refunds";
        assert_eq!(sqs.read(legacy), "refunds");
        assert_eq!(sqs.read("orders"), "orders");
        // A part that is not there fits no form.
        assert_eq!(
            sqs.read("arn:aws:sqs:eu-west-1::orders"),
            "arn:aws:sqs:eu-west-1::orders"
        );
        let s3 = entry("terraform", "aws_s3_bucket").unwrap();
        assert_eq!(s3.read("arn:aws:s3:::orders-archive"), "orders-archive");
        assert_eq!(
            s3.read("s3://orders-archive/2026/11/x.json"),
            "orders-archive"
        );
        assert_eq!(
            s3.read("https://orders.archive.s3.amazonaws.com"),
            "orders.archive"
        );
        let regional = "https://orders-archive.s3.eu-west-1.amazonaws.com/key";
        assert_eq!(s3.read(regional), "orders-archive");
        let path = "https://s3.eu-west-1.amazonaws.com/orders-archive/key";
        assert_eq!(s3.read(path), "orders-archive");
        // A form that stops short of the end fits only before a `/`.
        assert_eq!(s3.read("s3://a:b"), "s3://a:b");
    }

    /// Past `FORM_TEXT_MAX` a text is the name as it is, however many forms it would fit.
    #[test]
    fn a_text_past_the_form_ceiling_is_the_name_as_it_is() {
        let sqs = entry("terraform", "aws_sqs_queue").unwrap();
        let long = format!("arn:aws:sqs:eu-west-1:1:{}", "q".repeat(FORM_TEXT_MAX));
        assert_eq!(sqs.read(&long), long);
        let short = format!("arn:aws:sqs:eu-west-1:1:{}", "q".repeat(100));
        assert_eq!(sqs.read(&short), "q".repeat(100));
    }

    /// A text built to make the matching backtrack stays fast.
    #[test]
    fn matching_a_hostile_text_stays_bounded() {
        let s3 = entry("terraform", "aws_s3_bucket").unwrap();
        let text = format!("https://{}", ".s3".repeat(300));
        let started = std::time::Instant::now();
        let _ = s3.read(&text);
        assert!(started.elapsed() < std::time::Duration::from_secs(2));
    }
}
