//! Development measurements over public fixtures, never performance acceptance thresholds.

use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
    time::Instant,
};

use meada::engine::{Direction, Engine, Start};
use meada_core::render::tokens;
use rmcp::{
    ServiceExt,
    transport::{ConfigureCommandExt, TokioChildProcess},
};
use serde::Serialize;

use crate::common;

struct Suite {
    name: &'static str,
    fixtures: &'static [&'static str],
    setup: fn(&str) -> PathBuf,
    edit: &'static str,
    symbol: &'static str,
}

const SMOKE: Suite = Suite {
    name: "smoke",
    fixtures: &["ts-basic"],
    setup: |case| common::fixture_repo(case, "ts-basic", Some("https://example.com/acme/web.git")),
    edit: "src/math.ts",
    symbol: "src/math.ts::sum",
};

// Keep subjects explicit so the same answers are comparable between runs.
const SUITES: &[Suite] = &[
    Suite {
        name: "chain",
        fixtures: &[
            "chain-web",
            "chain-gateway",
            "chain-catalog",
            "chain-worker",
        ],
        setup: |case| common::drift_chain(case).0,
        edit: "src/dashboard.ts",
        symbol: "src/dashboard.ts::dashboard",
    },
    Suite {
        name: "conformance-ts-js",
        fixtures: &["conformance-ts-js"],
        setup: |case| {
            common::fixture_repo(
                case,
                "conformance-ts-js",
                Some("https://example.com/acme/conformance.git"),
            )
        },
        edit: "src/app.ts",
        symbol: "src/app.ts::main",
    },
    Suite {
        name: "conformance-python",
        fixtures: &["conformance-python"],
        setup: |case| {
            common::fixture_repo(
                case,
                "conformance-python",
                Some("https://example.com/acme/conformance.git"),
            )
        },
        edit: "geometry/shapes.py",
        symbol: "geometry/shapes.py::scale",
    },
    Suite {
        name: "conformance-go",
        fixtures: &["conformance-go", "conformance-go-ledger"],
        setup: |case| {
            common::fixture_repo(
                case,
                "conformance-go-ledger",
                Some("https://example.com/acme/ledger.git"),
            );
            common::fixture_repo(
                case,
                "conformance-go",
                Some("https://example.com/acme/conformance.git"),
            )
        },
        edit: "main.go",
        symbol: "main.go::main",
    },
    Suite {
        name: "conformance-rust",
        fixtures: &["conformance-rust"],
        setup: |case| {
            common::fixture_repo(
                case,
                "conformance-rust",
                Some("https://example.com/acme/conformance.git"),
            )
        },
        edit: "api/src/main.rs",
        symbol: "api/src/main.rs::main",
    },
    Suite {
        name: "xlang-python",
        fixtures: &["xlang-storefront", "xlang-inventory"],
        setup: |case| common::xlang(case).0,
        edit: "src/sync.ts",
        symbol: "src/sync.ts::syncInventory",
    },
    Suite {
        name: "xlang-go",
        fixtures: &["xlang-go-web", "xlang-go-api"],
        setup: |case| common::xlang_go(case).0,
        edit: "src/sync.ts",
        symbol: "src/sync.ts::syncOrders",
    },
    Suite {
        name: "xlang-rust",
        fixtures: &["xlang-rust-web", "xlang-rust-api"],
        setup: |case| common::xlang_rust(case).0,
        edit: "src/sync.ts",
        symbol: "src/sync.ts::syncOrders",
    },
    Suite {
        name: "conformance-java",
        fixtures: &["conformance-java"],
        setup: |case| {
            common::fixture_repo(
                case,
                "conformance-java",
                Some("https://example.com/acme/conformance.git"),
            )
        },
        edit: "maven/api/src/main/java/example/calls/Client.java",
        symbol: "maven/api/src/main/java/example/calls/Client.java::example.calls.Client.typed(Repo)",
    },
    Suite {
        name: "conformance-kotlin",
        fixtures: &["conformance-kotlin"],
        setup: |case| {
            common::fixture_repo(
                case,
                "conformance-kotlin",
                Some("https://example.com/acme/conformance.git"),
            )
        },
        edit: "src/main/kotlin/example/chains/Chains.kt",
        symbol: "src/main/kotlin/example/chains/Chains.kt::example.chains.use()",
    },
    Suite {
        name: "conformance-csharp",
        fixtures: &["conformance-csharp"],
        setup: |case| {
            common::fixture_repo(
                case,
                "conformance-csharp",
                Some("https://example.com/acme/conformance.git"),
            )
        },
        edit: "src/Acme.Core/Billing/Teller.cs",
        symbol: "src/Acme.Core/Billing/Teller.cs::Acme.Core.Billing.Teller.Run(Account,IAuditor,List<Account>)",
    },
    Suite {
        name: "conformance-swift",
        fixtures: &["conformance-swift"],
        setup: |case| {
            common::fixture_repo(
                case,
                "conformance-swift",
                Some("https://example.com/acme/conformance.git"),
            )
        },
        edit: "Sources/Checkout/Checkout.swift",
        symbol: "Sources/Checkout/Checkout.swift::Checkout.run()",
    },
    Suite {
        name: "conformance-swift-alias",
        fixtures: &["conformance-swift-alias"],
        setup: |case| {
            common::fixture_repo(
                case,
                "conformance-swift-alias",
                Some("https://example.com/acme/conformance.git"),
            )
        },
        edit: "Sources/App/main.swift",
        symbol: "Sources/App/main.swift::use(_:_:_:_:)",
    },
    Suite {
        name: "conformance-swift-app",
        fixtures: &["conformance-swift-app"],
        setup: |case| {
            common::fixture_repo(
                case,
                "conformance-swift-app",
                Some("https://example.com/acme/conformance.git"),
            )
        },
        edit: "Sources/Receivers.swift",
        symbol: "Sources/Receivers.swift::outside(_:_:_:_:)",
    },
    Suite {
        name: "conformance-swift-capitalized",
        fixtures: &["conformance-swift-capitalized"],
        setup: |case| {
            common::fixture_repo(
                case,
                "conformance-swift-capitalized",
                Some("https://example.com/acme/conformance.git"),
            )
        },
        edit: "Sources/Plain.swift",
        symbol: "Sources/Plain.swift::run()",
    },
    Suite {
        name: "conformance-swift-implicit",
        fixtures: &["conformance-swift-implicit"],
        setup: |case| {
            common::fixture_repo(
                case,
                "conformance-swift-implicit",
                Some("https://example.com/acme/conformance.git"),
            )
        },
        edit: "Sources/Implicit.swift",
        symbol: "Sources/Implicit.swift::Config.build()",
    },
    Suite {
        name: "conformance-swift-packages",
        fixtures: &["conformance-swift-packages"],
        setup: |case| {
            common::fixture_repo(
                case,
                "conformance-swift-packages",
                Some("https://example.com/acme/conformance.git"),
            )
        },
        edit: "one/Sources/App/App.swift",
        symbol: "one/Sources/App/App.swift::start()",
    },
    Suite {
        name: "xlang-java",
        fixtures: &["xlang-java-web", "xlang-java-api"],
        setup: |case| common::xlang_java(case).0,
        edit: "src/sync.ts",
        symbol: "src/sync.ts::syncOrders",
    },
    Suite {
        name: "xlang-kotlin",
        fixtures: &["xlang-kotlin-web", "xlang-kotlin-api"],
        setup: |case| common::xlang_kotlin(case).0,
        edit: "src/sync.ts",
        symbol: "src/sync.ts::syncOrders",
    },
    Suite {
        name: "xlang-csharp",
        fixtures: &["xlang-csharp-web", "xlang-csharp-api"],
        setup: |case| common::xlang_csharp(case).0,
        edit: "src/sync.ts",
        symbol: "src/sync.ts::syncOrders",
    },
    Suite {
        name: "xlang-swift",
        fixtures: &["xlang-swift-web", "xlang-swift-api"],
        setup: |case| common::xlang_swift(case).0,
        edit: "src/sync.ts",
        symbol: "src/sync.ts::syncOrders",
    },
];

fn fixture_coverage() {
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let mut expected: BTreeSet<String> = fs::read_dir(fixtures)
        .unwrap()
        .map(|entry| entry.unwrap())
        .filter(|entry| entry.file_type().unwrap().is_dir())
        .map(|entry| entry.file_name().into_string().unwrap())
        .filter(|name| name.starts_with("conformance-") || name.starts_with("xlang-"))
        .collect();
    expected.extend(
        [
            "chain-web",
            "chain-gateway",
            "chain-catalog",
            "chain-worker",
        ]
        .map(String::from),
    );
    let actual: BTreeSet<String> = SUITES
        .iter()
        .flat_map(|suite| suite.fixtures.iter().map(|name| String::from(*name)))
        .collect();
    assert_eq!(
        actual, expected,
        "every conformance/cross-language fixture and chain peer must be measured"
    );
}

#[test]
fn every_required_public_fixture_is_measured() {
    fixture_coverage();
}

#[derive(Serialize)]
struct IndexSize {
    fixture: String,
    bytes: u64,
}

#[derive(Serialize)]
struct Sample {
    suite: &'static str,
    repository_fixtures: &'static [&'static str],
    root_fixture: String,
    symbol: &'static str,
    cold_workspace_ms: f64,
    root_index_bytes: u64,
    workspace_index_bytes: u64,
    indexes: Vec<IndexSize>,
    edited_file: &'static str,
    one_file_workspace_reindex_ms: f64,
    answers: BTreeMap<&'static str, usize>,
}

#[derive(Serialize)]
struct ToolTokens {
    compact_list_result: usize,
    descriptions_and_schemas: usize,
}

#[derive(Serialize)]
struct Report {
    version: u8,
    meada_version: &'static str,
    profile: &'static str,
    tools: ToolTokens,
    suites: Vec<Sample>,
}

fn measure(suite: &Suite) -> Sample {
    let case = format!("benchmark_{}", suite.name);
    let _ = fs::remove_dir_all(Path::new(env!("CARGO_TARGET_TMPDIR")).join(&case));
    let root = (suite.setup)(&case);
    let mut repositories: Vec<_> = fs::read_dir(root.parent().unwrap())
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|dir| dir.join(".git").is_dir())
        .collect();
    repositories.sort_by_key(|dir| (dir != &root, dir.clone()));
    let mut actual: Vec<_> = repositories
        .iter()
        .map(|dir| dir.file_name().unwrap().to_str().unwrap())
        .collect();
    actual.sort_unstable();
    let mut expected = suite.fixtures.to_vec();
    expected.sort_unstable();
    assert_eq!(actual, expected, "{}: prepared repositories", suite.name);
    for repo in &repositories {
        assert!(
            !repo.join(".meada/index.db").exists(),
            "cold index already exists"
        );
    }

    let started = Instant::now();
    let engine = Engine::open(&root).unwrap();
    engine.refresh().unwrap();
    let cold_workspace_ms = started.elapsed().as_secs_f64() * 1000.0;
    // Reading every prepared repository's index also catches a peer silently left out.
    let indexes: Vec<_> = repositories
        .iter()
        .map(|repo| IndexSize {
            fixture: repo.file_name().unwrap().to_str().unwrap().into(),
            bytes: fs::metadata(repo.join(".meada/index.db")).unwrap().len(),
        })
        .collect();

    let before = meada_core::export::parse(&engine.export().unwrap()).unwrap();
    let file = root.join(suite.edit);
    let comment = if suite.edit.ends_with(".py") {
        "\n# benchmark edit\n"
    } else {
        "\n// benchmark edit\n"
    };
    fs::write(&file, fs::read_to_string(&file).unwrap() + comment).unwrap();
    let started = Instant::now();
    engine.refresh().unwrap();
    let one_file_workspace_reindex_ms = started.elapsed().as_secs_f64() * 1000.0;
    let after = meada_core::export::parse(&engine.export().unwrap()).unwrap();
    assert_eq!(
        before.files.len(),
        after.files.len(),
        "reindex changed the file set"
    );
    let changed: Vec<_> = before
        .files
        .iter()
        .zip(&after.files)
        .filter_map(|(a, b)| {
            assert_eq!(a.path, b.path, "reindex changed the file set");
            (a.hash != b.hash).then_some(a.path.as_str())
        })
        .collect();
    assert_eq!(
        changed,
        [suite.edit],
        "reindex must observe exactly the source edit"
    );

    // Fixed queries with default budgets. A missing subject is a harness error, not a cheap answer.
    let search = engine.search("", None, None).unwrap();
    let context = engine.context(suite.symbol, None).unwrap();
    let trace = engine
        .trace(Start::From(suite.symbol), Direction::Down, None)
        .unwrap();
    assert!(
        !search.is_empty() && !search.starts_with("no matches"),
        "empty search"
    );
    assert!(
        context.contains("\ncommunity:"),
        "context is not a symbol answer: {}",
        suite.name
    );
    assert!(
        trace.contains("\nchain:")
            && trace
                .lines()
                .next()
                .unwrap()
                .ends_with(suite.symbol.rsplit("::").next().unwrap()),
        "trace is not a symbol answer: {}",
        suite.name
    );
    assert!(
        !trace.contains("missing peer:"),
        "trace has a missing peer: {}",
        suite.name
    );
    Sample {
        suite: suite.name,
        repository_fixtures: suite.fixtures,
        root_fixture: root.file_name().unwrap().to_str().unwrap().into(),
        symbol: suite.symbol,
        cold_workspace_ms,
        root_index_bytes: indexes[0].bytes,
        workspace_index_bytes: indexes.iter().map(|i| i.bytes).sum(),
        indexes,
        edited_file: suite.edit,
        one_file_workspace_reindex_ms,
        answers: [
            ("search", tokens(&search)),
            ("context", tokens(&context)),
            ("trace", tokens(&trace)),
        ]
        .into(),
    }
}

async fn tool_tokens(case: &str) -> ToolTokens {
    let root = common::fixture_repo(case, "ts-basic", Some("https://example.com/acme/web.git"));
    let transport = TokioChildProcess::new(
        tokio::process::Command::new(env!("CARGO_BIN_EXE_meada")).configure(|command| {
            command.arg("mcp").current_dir(root);
        }),
    )
    .unwrap();
    let client = ().serve(transport).await.unwrap();
    let result = client.list_tools(None).await.unwrap();
    assert!(
        result.next_cursor.is_none(),
        "tool list unexpectedly paginated"
    );
    let mut budgeted = String::new();
    for tool in &result.tools {
        budgeted.push_str(tool.description.as_deref().unwrap_or(""));
        budgeted.push_str(&serde_json::to_string(&*tool.input_schema).unwrap());
    }
    let tools = ToolTokens {
        compact_list_result: tokens(&serde_json::to_string(&result).unwrap()),
        descriptions_and_schemas: tokens(&budgeted),
    };
    client.cancel().await.unwrap();
    tools
}

fn summary(report: &Report) -> String {
    let mut text = format!(
        "# Public fixture benchmark\n\nMeada {}, {} profile. Estimates use the trunk's `render::tokens`.\n\n\
        | Tool-list measure | Tokens |\n| --- | ---: |\n\
        | Compact tools/list result (no JSON-RPC framing) | {} |\n\
        | Descriptions and input schemas (existing budget metric) | {} |\n\n\
        | Suite | Repositories | Cold workspace ms | Root index bytes | Workspace index bytes | One-file workspace reindex ms | Search tokens | Context tokens | Trace tokens |\n\
        | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |\n",
        report.meada_version,
        report.profile,
        report.tools.compact_list_result,
        report.tools.descriptions_and_schemas
    );
    for sample in &report.suites {
        text += &format!(
            "| {} | {} | {:.3} | {} | {} | {:.3} | {} | {} | {} |\n",
            sample.suite,
            sample.indexes.len(),
            sample.cold_workspace_ms,
            sample.root_index_bytes,
            sample.workspace_index_bytes,
            sample.one_file_workspace_reindex_ms,
            sample.answers["search"],
            sample.answers["context"],
            sample.answers["trace"]
        );
    }
    text += "\nCold means indexes absent, not an emptied OS page cache. Fixture setup and report I/O are excluded.\n\
        Reindex measures workspace refresh after one root source file changes. Sizes are logical index.db\n\
        file lengths after cold indexing, with root and workspace totals distinguished. Timings and token\n\
        counts are informational here; existing tests enforce token budgets. JSON lists every index and\n\
        the fixed symbol and edited file per suite. Only public fixtures are used.\n";
    text
}

async fn run(case: &str, suites: &[Suite], out: &Path) {
    let report = Report {
        version: 1,
        meada_version: env!("CARGO_PKG_VERSION"),
        profile: if cfg!(debug_assertions) {
            "debug"
        } else {
            "release"
        },
        tools: tool_tokens(case).await,
        suites: suites.iter().map(measure).collect(),
    };
    fs::create_dir_all(out).unwrap();
    fs::write(
        out.join("results.json"),
        serde_json::to_string_pretty(&report).unwrap() + "\n",
    )
    .unwrap();
    fs::write(out.join("summary.md"), summary(&report)).unwrap();
}

#[tokio::test]
async fn a_public_fixture_run_writes_machine_and_summary_reports() {
    let out = Path::new(env!("CARGO_TARGET_TMPDIR")).join("benchmark_smoke_report");
    let _ = fs::remove_dir_all(&out);
    run("benchmark_smoke_tools", &[SMOKE], &out).await;
    let report: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(out.join("results.json")).unwrap()).unwrap();
    assert_eq!(report["version"], 1);
    assert_eq!(
        report["profile"],
        if cfg!(debug_assertions) {
            "debug"
        } else {
            "release"
        }
    );
    let sample = &report["suites"][0];
    assert_eq!(sample["root_fixture"], "ts-basic");
    assert_eq!(sample["answers"]["trace"], 32);
    assert_eq!(sample["indexes"].as_array().unwrap().len(), 1);
    assert_eq!(sample["root_index_bytes"], sample["workspace_index_bytes"]);
    assert!(
        report["tools"]["compact_list_result"].as_u64().unwrap()
            > report["tools"]["descriptions_and_schemas"]
                .as_u64()
                .unwrap()
    );
    let summary = fs::read_to_string(out.join("summary.md")).unwrap();
    assert!(summary.contains("| smoke | 1 |"));
    for scope in [
        "Cold workspace ms",
        "Root index bytes",
        "Workspace index bytes",
        "One-file workspace reindex ms",
    ] {
        assert!(summary.contains(scope), "{scope}");
    }
}

#[test]
#[should_panic(expected = "context is not a symbol answer")]
fn a_bad_fixed_query_is_a_harness_error() {
    measure(&Suite {
        name: "invalid-query",
        symbol: "src/math.ts::gone",
        ..SMOKE
    });
}

#[tokio::test]
#[ignore = "development benchmark: run with --release --ignored --exact"]
async fn records_public_fixture_benchmark() {
    fixture_coverage();
    let out = std::env::var_os("MEADA_BENCH_OUT")
        .map(PathBuf::from)
        .unwrap_or_else(|| Path::new(env!("CARGO_TARGET_TMPDIR")).join("public-fixture-benchmark"));
    run("benchmark_full_tools", SUITES, &out).await;
    println!("benchmark report: {}", out.join("summary.md").display());
}
