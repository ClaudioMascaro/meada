//! Answer-sized reads (#396): `search`, `context` and `trace` read from the indexes the rows of
//! their answer, and answer as if they had read every row. Each query runs both ways over the
//! fixture workspaces, symbols, call sites, contracts and every process of them, and the
//! answers must be the same bytes: `with_every_row` reads every row, as `meada export` does.

use crate::common;

use std::fs;
use std::path::{Path, PathBuf};

use common::{decisions, fixture_repo};
use meada::engine::{Direction, Engine, Start, with_every_row};
use serde_json::Value;

/// No cut hides a difference.
const BUDGET: Option<usize> = Some(1_000_000);

/// The `search` kinds, `None` being every kind but the early ones.
const KINDS: [Option<&str>; 16] = [
    None,
    Some("symbol"),
    Some("contract"),
    Some("route"),
    Some("resource"),
    Some("channel"),
    Some("document"),
    Some("package"),
    Some("drift"),
    Some("community"),
    Some("process"),
    Some("duplicate"),
    Some("ambiguous"),
    Some("unresolved"),
    Some("value"),
    Some("iac"),
];

/// At most this many symbols, call sites and contracts of a repository are queried, spread
/// over all of them: every row of a large fixture costs the suite minutes and finds nothing more.
const SAMPLE: usize = 24;

/// At most `SAMPLE` of `all`, spread over them in order.
fn sample(all: Vec<String>) -> Vec<String> {
    let step = all.len().div_ceil(SAMPLE).max(1);
    all.into_iter().step_by(step).collect()
}

/// The strings of `field` in every object of the array `list` of `export`.
fn each(export: &Value, list: &str, field: &str) -> Vec<String> {
    let rows = export[list].as_array().into_iter().flatten();
    rows.filter_map(|row| row[field].as_str().map(String::from))
        .collect()
}

/// Asserts that every query from `repo` answers the same reading the rows of its answer and
/// reading every row.
fn answers_alike(repo: &Path) {
    let engine = Engine::open(repo).unwrap();
    let export: Value = serde_json::from_str(&engine.export().unwrap()).unwrap();
    let symbols = sample(each(&export, "symbols", "id"));
    let calls = sample(each(&export, "calls", "id"));
    let mut contracts = each(&export, "providers", "contract");
    contracts.extend(each(&export, "consumers", "contract"));
    contracts.sort();
    contracts.dedup();
    let contracts = sample(contracts);
    let infrastructure = sample(
        export["iac_nodes"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|node| {
                format!(
                    "{} (root {})",
                    node["identity"].as_str().unwrap(),
                    node["root"].as_str().unwrap()
                )
            })
            .collect(),
    );
    let processes: Vec<String> = (each(&export, "entry_points", "identity").into_iter())
        .map(|e| format!("process::{e}"))
        .collect();
    let names: Vec<String> = (symbols.iter())
        .filter_map(|id| id.rsplit("::").next())
        .map(String::from)
        .collect();

    let alike = |what: &str, ask: &dyn Fn() -> String| {
        let answer = ask();
        let whole = with_every_row(ask);
        assert!(
            answer == whole,
            "{} {what}: the rows of the answer give\n{answer}\nevery row gives\n{whole}",
            repo.display()
        );
    };
    let search = |query: &str, kind: Option<&str>| {
        let ask = || format!("{:?}", engine.search(query, kind, BUDGET));
        alike(&format!("search {query:?} {kind:?}"), &ask);
    };
    for kind in KINDS {
        search("", kind);
        for name in names.iter().take(3) {
            search(name, kind);
        }
    }
    let context = |symbol: &str| {
        let ask = || format!("{:?}", engine.context(symbol, BUDGET));
        alike(&format!("context {symbol}"), &ask);
    };
    for symbol in symbols
        .iter()
        .chain(&names)
        .chain(&contracts)
        .chain(&infrastructure)
    {
        context(symbol);
    }
    let trace = |start: Start, direction: Direction| {
        let ask = || format!("{:?}", engine.trace(start, direction, BUDGET));
        alike(&format!("trace {start:?} {direction:?}"), &ask);
    };
    // Short names too: `trace::starts` finds them by suffix, and lists several fits.
    let starts = (symbols
        .iter()
        .chain(&names)
        .chain(&calls)
        .chain(&contracts)
        .chain(&infrastructure)
        .chain(&processes))
    .map(|from| Start::From(from));
    for start in starts.chain([Start::Uncommitted]) {
        trace(start, Direction::Down);
        trace(start, Direction::Up);
    }
    // An uncommitted edit inside a symbol, so the trace from the changes starts somewhere.
    let Some((path, start)) = first_span(&export) else {
        return;
    };
    let file = repo.join(&path);
    let Ok(source) = fs::read_to_string(&file) else {
        return;
    };
    if !source.is_char_boundary(start) {
        return;
    }
    // A space at the end of the symbol's first line.
    let end = source[start..]
        .find('\n')
        .map_or(source.len(), |at| start + at);
    fs::write(&file, format!("{} {}", &source[..end], &source[end..])).unwrap();
    trace(Start::Uncommitted, Direction::Down);
    trace(Start::Uncommitted, Direction::Up);
    fs::write(&file, source).unwrap();
}

/// The path and start offset (in bytes) of the first symbol of `export`.
fn first_span(export: &Value) -> Option<(String, usize)> {
    let symbols = export["symbols"].as_array()?;
    symbols.iter().find_map(|s| {
        let path = s["span"]["path"].as_str()?;
        let start = usize::try_from(s["span"]["start"].as_i64()?).ok()?;
        Some((path.to_string(), start))
    })
}

#[test]
fn the_chain_answers_alike() {
    let (web, gateway, catalog) = common::chain("partial_chain");
    decisions(&web, "chain-web");
    for repo in [&web, &gateway, &catalog, &common::worker(&web)] {
        answers_alike(repo);
    }
}

#[test]
fn the_documented_and_drifting_chains_answer_alike() {
    let (web, gateway, catalog) = common::documented_chain("partial_documented");
    for repo in [web, gateway, catalog] {
        answers_alike(&repo);
    }
    let (web, gateway, catalog) = common::drift_chain("partial_drift");
    for repo in [web, gateway, catalog] {
        answers_alike(&repo);
    }
    answers_alike(&common::loose_chain("partial_loose"));
}

#[test]
fn the_languages_answer_alike() {
    type Pair = fn(&str) -> (PathBuf, PathBuf);
    let pairs: [(&str, Pair); 7] = [
        ("partial_xlang", common::xlang),
        ("partial_xlang_rust", common::xlang_rust),
        ("partial_xlang_go", common::xlang_go),
        ("partial_xlang_java", common::xlang_java),
        ("partial_xlang_kotlin", common::xlang_kotlin),
        ("partial_xlang_csharp", common::xlang_csharp),
        ("partial_xlang_swift", common::xlang_swift),
    ];
    for (case, pair) in pairs {
        let (a, b) = pair(case);
        answers_alike(&a);
        answers_alike(&b);
    }
}

#[test]
fn channels_packages_and_documents_answer_alike() {
    let case = "partial_channels";
    for name in ["worker", "billing", "api"] {
        let origin = format!("https://example.com/channels/{name}.git");
        let repo = fixture_repo(case, &format!("channels-{name}"), Some(&origin));
        if name == "api" {
            answers_alike(&repo);
        }
    }
    let case = "partial_packages";
    let repos: Vec<PathBuf> = ["shared", "web", "api", "mono", "fork"]
        .iter()
        .map(|name| {
            let origin = format!("https://example.com/pkg/{name}.git");
            fixture_repo(case, &format!("pkg-{name}"), Some(&origin))
        })
        .collect();
    for repo in &repos {
        answers_alike(repo);
    }
    let case = "partial_asyncapi";
    let events = "https://example.com/asyncapi/events.git";
    fixture_repo(case, "asyncapi-events", Some(events));
    let services = "https://example.com/asyncapi/services.git";
    answers_alike(&fixture_repo(case, "asyncapi-services", Some(services)));
}

#[test]
fn contract_trace_call_owners_answer_alike() {
    for (fixture, contract, owner) in [
        ("aws-sdk", "channel::sns::orders", "publish"),
        ("aws-sdk-python", "channel::sns::orders", "publish"),
        ("aws-sdk-matrix", "channel::eventbridge::default", "run"),
        (
            "aws-sdk-batches",
            "channel::eventbridge::orders",
            "Batches.send()",
        ),
    ] {
        let repo = fixture_repo("partial_provider_owners", fixture, None);
        let engine = Engine::open(&repo).unwrap();
        let ask = || {
            engine
                .trace(Start::From(contract), Direction::Down, BUDGET)
                .unwrap()
        };
        let answer = ask();
        let whole = with_every_row(ask);
        assert!(
            whole.lines().any(|line| line.ends_with(owner)),
            "{fixture}: {whole}"
        );
        assert_eq!(answer, whole, "{fixture}: contract-side call owners");
    }
}

/// Terraform JSON nodes must survive the partial path, including their references and
/// root-specific context and trace starts. The mutation named in PR #711 drops only
/// JSON nodes from partial exports and makes this equivalence fail.
#[test]
fn terraform_json_infrastructure_queries_answer_alike() {
    for fixture in [
        "terraform-json",
        "terraform-json-contracts",
        "terraform-json-repeated",
    ] {
        let repo = fixture_repo("partial_terraform_json", fixture, None);
        answers_alike(&repo);
    }
}

/// Every fixture alone, in four shards so they run side by side.
fn every_fixture_alone(shard: usize) {
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let mut names: Vec<String> = fs::read_dir(fixtures)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_dir())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    for name in names.iter().skip(shard).step_by(4) {
        let repo = fixture_repo(&format!("partial_alone_{shard}"), name, None);
        // A directory of data (decisions files, schema samples) or one a test means to be
        // refused is no workspace to query.
        if Engine::open(&repo).and_then(|e| e.export()).is_ok() {
            answers_alike(&repo);
        }
    }
}

#[test]
fn every_fixture_alone_answers_alike_0() {
    every_fixture_alone(0);
}

#[test]
fn every_fixture_alone_answers_alike_1() {
    every_fixture_alone(1);
}

#[test]
fn every_fixture_alone_answers_alike_2() {
    every_fixture_alone(2);
}

#[test]
fn every_fixture_alone_answers_alike_3() {
    every_fixture_alone(3);
}
