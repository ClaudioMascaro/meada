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

pub(crate) const LISTED: [(&str, &str); 5] = [
    (WORKFLOWS, "ci-workflows"),
    ("github.com/acme/wrapper", "ci-wrapper"),
    (APP_A, "ci-app-a"),
    (APP_B, "ci-app-b"),
    (APP_C, "ci-app-c"),
];

pub(crate) const PUBLISH: &str = ".github/workflows/publish.yml";
pub(crate) const CONTRACT: &str = "ci::github.com/acme/workflows/.github/workflows/publish.yml";
pub(crate) const JOB: &str = ".github/workflows/ci.yml::jobs.publish";

pub(crate) fn listed(case: &str, entries: &[(&str, &str)]) -> PathBuf {
    let dir = reproduction(case)[0].parent().unwrap().to_path_buf();
    list(&dir, entries, false);
    dir
}

pub(crate) fn about_the_call() -> [(&'static str, Value); 3] {
    [
        ("context", json!({"symbol": JOB})),
        ("trace", json!({"from": JOB})),
        ("search", json!({"query": "publish", "kind": "contract"})),
    ]
}

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

fn served(case: &str, entries: &[(&str, &str)]) -> (PathBuf, Server) {
    let dir = listed(case, entries);
    assert!(run(&dir, "index").2, "index");
    let args = ["--listen", "127.0.0.1:0", "--workspace", MANIFEST];
    let server = Server::start_args(&dir, &args, &[]);
    (dir, server)
}

async fn stdio(root: &Path) -> RunningService<RoleClient, ()> {
    let transport = TokioChildProcess::new(
        tokio::process::Command::new(env!("CARGO_BIN_EXE_meada")).configure(|c| {
            c.arg("mcp").current_dir(root);
        }),
    )
    .unwrap();
    ().serve(transport).await.unwrap()
}

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
        (WORKFLOWS, "context", json!({"symbol": PUBLISH})),
        (APP_A, context.0, context.1),
        (APP_A, "context", json!({"symbol": CONTRACT})),
        (WORKFLOWS, "trace", up(PUBLISH)),
        (WORKFLOWS, "trace", up(&node)),
        (WORKFLOWS, "trace", up(CONTRACT)),
        (APP_A, down.0, down.1),
        (
            APP_C,
            "trace",
            json!({"from": format!("{APP_C}::.github/workflows/release.yml::jobs.release")}),
        ),
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
        let answer = text(&over_http);
        assert!(
            answer.contains("acme/") && !answer.contains("pass one of"),
            "{identity} {tool} {arguments}: {answer}"
        );
    }
}

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
    let links = export["links"].to_string();
    assert!(
        links.contains("ci::github.com/acme/app-a/tools/local"),
        "{links}"
    );
}
