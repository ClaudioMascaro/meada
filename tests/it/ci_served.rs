//! The CI kind in server mode (#829, ADR 0028): `meada mcp --listen --workspace` answers every
//! flow of the kind as the stdio server does over the reproduction of #816, and a manifest that
//! does not list the provider's repository gives no link to it and no consumer of it. The grants
//! of `oidc.rs` and the console of `ui/playground.rs` ask the same questions.

use std::path::{Path, PathBuf};

use rmcp::{
    RoleClient, ServiceExt,
    service::RunningService,
    transport::{ConfigureCommandExt, TokioChildProcess},
};
use serde_json::{Value, json};

use crate::ci_calls::reproduction;
use crate::manifest::{MANIFEST, list, run, run_export};
use crate::server::{Server, call, text};

pub(crate) const WORKFLOWS: &str = "github.com/acme/workflows";
pub(crate) const APP_A: &str = "github.com/acme/app-a";
pub(crate) const APP_B: &str = "github.com/acme/app-b";
pub(crate) const APP_C: &str = "github.com/acme/app-c";

/// The reproduction's repositories as a manifest lists them (identity, directory), in the order
/// of `reproduction`.
pub(crate) const LISTED: [(&str, &str); 5] = [
    (WORKFLOWS, "ci-workflows"),
    ("github.com/acme/wrapper", "ci-wrapper"),
    (APP_A, "ci-app-a"),
    (APP_B, "ci-app-b"),
    (APP_C, "ci-app-c"),
];

/// The reusable pipeline of `WORKFLOWS` the callers share, its contract, and the CI job of
/// `APP_A` that calls it.
pub(crate) const PUBLISH: &str = ".github/workflows/publish.yml";
pub(crate) const CONTRACT: &str = "ci::github.com/acme/workflows/.github/workflows/publish.yml";
pub(crate) const JOB: &str = ".github/workflows/ci.yml::jobs.publish";

/// The reproduction in the case directory `case`, with a manifest that lists `entries` of it; not
/// indexed. Returns the case directory.
pub(crate) fn listed(case: &str, entries: &[(&str, &str)]) -> PathBuf {
    let dir = reproduction(case)[0].parent().unwrap().to_path_buf();
    list(&dir, entries, false);
    dir
}

/// What a caller is asked about its call to `PUBLISH`: `context` and `trace` from `JOB`, and the
/// contracts that match the pipeline's name.
pub(crate) fn about_the_call() -> [(&'static str, Value); 3] {
    [
        ("context", json!({"symbol": JOB})),
        ("trace", json!({"from": JOB})),
        ("search", json!({"query": "publish", "kind": "contract"})),
    ]
}

/// Fails when `answer` holds a CI side or a link of `WORKFLOWS`, or names it as a missing peer.
/// The `uses` a caller wrote stays: it is the caller's own value.
pub(crate) fn no_provider(answer: &str) {
    for leak in [
        "ci::github.com/acme/workflows",
        "github.com/acme/workflows ",
        "consumes ",
        "→[",
        "missing peer",
    ] {
        assert!(!answer.contains(leak), "{leak} in: {answer}");
    }
}

/// `entries` of the reproduction listed and indexed in the case directory `case`, served over
/// HTTP. Returns the case directory and the server.
fn served(case: &str, entries: &[(&str, &str)]) -> (PathBuf, Server) {
    let dir = listed(case, entries);
    assert!(run(&dir, "index").2, "index");
    let args = ["--listen", "127.0.0.1:0", "--workspace", MANIFEST];
    let server = Server::start_args(&dir, &args, &[]);
    (dir, server)
}

/// A stdio client of `meada mcp` in the checkout at `root`.
async fn stdio(root: &Path) -> RunningService<RoleClient, ()> {
    let transport = TokioChildProcess::new(
        tokio::process::Command::new(env!("CARGO_BIN_EXE_meada")).configure(|c| {
            c.arg("mcp").current_dir(root);
        }),
    )
    .unwrap();
    ().serve(transport).await.unwrap()
}

/// The flows of the kind: the repository asked, the tool and its arguments.
fn flows() -> Vec<(&'static str, &'static str, Value)> {
    let node = format!("{WORKFLOWS}::{PUBLISH}::.");
    let up = |from: &str| json!({"from": from, "direction": "up"});
    let [context, down, contracts] = about_the_call();
    vec![
        (
            WORKFLOWS,
            "search",
            json!({"query": "publish", "kind": "ci"}),
        ),
        (APP_A, contracts.0, contracts.1),
        // The consumers of a provider, a consumer, and the contract itself.
        (WORKFLOWS, "context", json!({"symbol": PUBLISH})),
        (APP_A, context.0, context.1),
        (APP_A, "context", json!({"symbol": CONTRACT})),
        // The three starts of one chain, the chain down from a caller, and from the last one.
        (WORKFLOWS, "trace", up(PUBLISH)),
        (WORKFLOWS, "trace", up(&node)),
        (WORKFLOWS, "trace", up(CONTRACT)),
        (APP_A, down.0, down.1),
        (
            APP_C,
            "trace",
            json!({"from": ".github/workflows/release.yml::jobs.release"}),
        ),
        // The absent mark and an unresolved CI step with its cause.
        (
            APP_A,
            "context",
            json!({"symbol": ".github/workflows/ci.yml::jobs.internal"}),
        ),
        (
            APP_A,
            "context",
            json!({"symbol": ".github/workflows/forms.yml::jobs[\"no-checkout\"].steps[0]"}),
        ),
    ]
}

#[tokio::test]
async fn mcp_over_http_answers_every_flow_of_the_kind_as_stdio_does() {
    let (_dir, server) = served("ci_served_http", &LISTED);
    let local = reproduction("ci_served_stdio");
    for (identity, tool, arguments) in flows() {
        let at = LISTED.iter().position(|(listed, _)| *listed == identity);
        let http = server.client(&format!("/{identity}")).await;
        let over_http = http.call_tool(call(tool, arguments.clone())).await.unwrap();
        http.cancel().await.unwrap();
        let client = stdio(&local[at.unwrap()]).await;
        let over_stdio = (client.call_tool(call(tool, arguments.clone())).await).unwrap();
        client.cancel().await.unwrap();
        assert_eq!(
            text(&over_http),
            text(&over_stdio),
            "{identity} {tool} {arguments}"
        );
        // No flow is compared empty.
        assert!(
            text(&over_http).contains("acme/"),
            "{identity} {tool} {arguments}"
        );
    }
}

/// The workspace filter: what a node serves is what its manifest lists, so two callers served
/// without the repository of the pipelines hold no CI side that names it, in `context`, `trace`,
/// `search` and the export the console's graph draws.
#[tokio::test]
async fn a_workspace_without_the_provider_s_repository_shows_no_link_and_no_consumer() {
    let entries: Vec<_> = (LISTED.into_iter())
        .filter(|(identity, _)| [APP_A, APP_B].contains(identity))
        .collect();
    let (dir, server) = served("ci_served_bounded", &entries);
    let client = server.client(&format!("/{APP_A}")).await;
    for (tool, arguments) in about_the_call() {
        let answer = client.call_tool(call(tool, arguments)).await.unwrap();
        no_provider(text(&answer));
        // The caller answers: an error would pass otherwise.
        if tool != "search" {
            assert!(text(&answer).contains("jobs.publish"), "{}", text(&answer));
        }
    }
    client.cancel().await.unwrap();
    let (export, stderr, code) = run_export(&dir, APP_A);
    assert_eq!(code, Some(0), "{stderr}");
    let export: Value = serde_json::from_str(&export).unwrap();
    for section in ["consumers", "links"] {
        for row in export[section].as_array().unwrap() {
            let contract = row["contract"].as_str().unwrap_or("");
            assert!(!contract.contains("acme/workflows"), "{section}: {row}");
        }
    }
    // The calls inside the repository keep their links.
    let links = export["links"].to_string();
    assert!(
        links.contains("ci::github.com/acme/app-a/tools/local"),
        "{links}"
    );
}
