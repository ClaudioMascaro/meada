use crate::common;

use std::{
    sync::atomic::{AtomicUsize, Ordering},
    time::Duration,
};

use common::{assert_golden, fixture_repo};
use rmcp::{
    ClientHandler, ClientLifecycleMode, ClientServiceExt, ServiceExt,
    model::{
        CallToolRequestParams, CallToolResponse, CallToolResult, ClientCapabilities, ClientConfig,
        ErrorData, Implementation, ProtocolVersion,
    },
    service::{RequestContext, RoleClient, RunningService},
    transport::{ConfigureCommandExt, TokioChildProcess},
};

#[expect(
    deprecated,
    reason = "roots are deprecated by SEP-2577; still how Claude Code gives the session's directory, now through the multi round-trip request on the 2026-07-28 era"
)]
use rmcp::model::{ListRootsResult, Root};

const LEGACY: ProtocolVersion = ProtocolVersion::V_2025_11_25;
const MODERN: ProtocolVersion = ProtocolVersion::V_2026_07_28;

async fn connect<H: ClientHandler>(
    handler: H,
    transport: TokioChildProcess,
) -> RunningService<RoleClient, H> {
    let version = handler.get_info().protocol_version;
    if version < MODERN {
        return handler.serve(transport).await.unwrap();
    }
    let lifecycle = ClientLifecycleMode::Discover {
        preferred_versions: vec![version],
    };
    handler
        .serve_with_lifecycle(transport, lifecycle)
        .await
        .unwrap()
}

#[expect(
    deprecated,
    reason = "roots are deprecated by SEP-2577; still how Claude Code gives the session's directory"
)]
fn roots_config(version: ProtocolVersion) -> ClientConfig {
    let mut config = ClientConfig::new(
        ClientCapabilities::builder().enable_roots().build(),
        Implementation::from_build_env(),
    );
    config.protocol_version = version;
    config
}

#[tokio::test]
async fn serves_three_tools_over_stdio_within_the_description_budget() {
    let repo = fixture_repo(
        "mcp_smoke",
        "ts-basic",
        Some("https://example.com/acme/web.git"),
    );
    let transport = TokioChildProcess::new(
        tokio::process::Command::new(env!("CARGO_BIN_EXE_meada")).configure(|c| {
            c.arg("mcp").current_dir(&repo);
        }),
    )
    .unwrap();
    let client = ().serve(transport).await.unwrap();

    let tools = client.list_all_tools().await.unwrap();
    let mut names: Vec<&str> = tools.iter().map(|t| &*t.name).collect();
    names.sort();
    assert_eq!(names, ["context", "search", "trace"]);
    let mut listed = tools.clone();
    listed.sort_by(|a, b| a.name.cmp(&b.name));
    assert_golden(
        "mcp.tools.json",
        &format!("{}\n", serde_json::to_string_pretty(&listed).unwrap()),
    );
    let bytes: usize = tools
        .iter()
        .map(|t| {
            t.description.as_deref().unwrap_or("").len()
                + serde_json::to_string(&*t.input_schema).unwrap().len()
        })
        .sum();
    assert!(
        bytes.div_ceil(4) <= 1_000,
        "tool list costs {} tokens",
        bytes.div_ceil(4)
    );
    assert_eq!(meada_core::render::DEFAULT_BUDGET, 2_000);

    let answer = client
        .call_tool(
            CallToolRequestParams::new("trace").with_arguments(
                serde_json::json!({"from": "sum"})
                    .as_object()
                    .unwrap()
                    .clone(),
            ),
        )
        .await
        .unwrap();
    let text = answer.content.first().and_then(|c| c.as_text()).unwrap();
    assert_eq!(
        text.text,
        "example.com/acme/web src/math.ts:5 sum\n  example.com/acme/web src/math.ts:1 add\n\
         chain: exact · 0 hops · 1 repos · 32 tokens\n"
    );
    client.cancel().await.unwrap();
}

#[tokio::test]
async fn pipelined_calls_all_get_a_result() {
    let repo = fixture_repo(
        "mcp_pipelined",
        "ts-basic",
        Some("https://example.com/acme/web.git"),
    );
    let (transport, _stderr) = server_in(&repo);
    let client = ().serve(transport).await.unwrap();
    let calls = [
        ("search", serde_json::json!({"query": "sum"})),
        ("context", serde_json::json!({"symbol": "sum"})),
        ("trace", serde_json::json!({"from": "sum"})),
    ];
    for round in 0..3 {
        let math = repo.join("src/math.ts");
        let source = std::fs::read_to_string(&math).unwrap();
        std::fs::write(&math, format!("{source}// round {round}\n")).unwrap();
        let mut in_flight = tokio::task::JoinSet::new();
        for (tool, arguments) in calls.iter().cycle().take(9) {
            let peer = client.peer().clone();
            let params = CallToolRequestParams::new(*tool)
                .with_arguments(arguments.as_object().unwrap().clone());
            in_flight.spawn(async move { (params.name.clone(), peer.call_tool(params).await) });
        }
        while let Some(reply) = in_flight.join_next().await {
            let (tool, answer) = reply.unwrap();
            let answer = answer.unwrap();
            let text = text_of(&answer);
            assert_ne!(answer.is_error, Some(true), "{tool}: {text}");
            assert!(text.contains("sum"), "{tool}: {text}");
        }
    }
    client.cancel().await.unwrap();
}

#[tokio::test]
async fn calls_past_the_bound_wait_their_turn_and_each_gets_its_own_reply() {
    let repo = fixture_repo(
        "mcp_calls_in_flight",
        "ts-basic",
        Some("https://example.com/acme/web.git"),
    );
    let (transport, _stderr) = server_in(&repo);
    let client = ().serve(transport).await.unwrap();
    let calls = [
        ("search", serde_json::json!({"query": "sum"})),
        ("search", serde_json::json!({"query": "add"})),
        ("context", serde_json::json!({"symbol": "sum"})),
        ("trace", serde_json::json!({"from": "sum"})),
        (
            "trace",
            serde_json::json!({"from": "add", "direction": "up"}),
        ),
    ];
    let params = |n: usize| {
        let (tool, arguments) = &calls[n % calls.len()];
        CallToolRequestParams::new(*tool).with_arguments(arguments.as_object().unwrap().clone())
    };
    let mut alone = Vec::new();
    for n in 0..calls.len() {
        let answer = client.call_tool(params(n)).await.unwrap();
        alone.push(text_of(&answer).to_string());
    }
    let held = rusqlite::Connection::open(repo.join(".meada/index.db")).unwrap();
    held.execute_batch("BEGIN EXCLUSIVE").unwrap();
    let release = tokio::spawn(async move {
        tokio::time::sleep(Duration::from_secs(12)).await;
        held.execute_batch("ROLLBACK").unwrap();
    });

    let mut in_flight = tokio::task::JoinSet::new();
    for n in 0..12 {
        let (peer, params) = (client.peer().clone(), params(n));
        in_flight.spawn(async move { (n, peer.call_tool(params).await.unwrap()) });
    }
    let replies = tokio::time::timeout(Duration::from_secs(120), in_flight.join_all())
        .await
        .expect("every call is answered");
    release.await.unwrap();

    let (locked, answered): (Vec<_>, Vec<_>) =
        (replies.into_iter()).partition(|(_, answer)| answer.is_error == Some(true));
    assert_eq!(locked.len(), 4, "{locked:?}");
    for (n, answer) in &locked {
        assert!(
            text_of(answer).contains("locked"),
            "call {n}: {}",
            text_of(answer)
        );
    }
    for (n, answer) in &answered {
        assert_eq!(text_of(answer), alone[n % calls.len()], "call {n}");
    }
    client.cancel().await.unwrap();
}

#[tokio::test]
async fn answers_about_the_clients_first_root_not_its_cwd() {
    let cwd = fixture_repo(
        "mcp_roots_cwd",
        "ts-basic",
        Some("https://example.com/acme/cwd.git"),
    );
    let root = fixture_repo(
        "mcp roots root",
        "ts-basic",
        Some("https://example.com/acme/root.git"),
    );
    let transport = TokioChildProcess::new(
        tokio::process::Command::new(env!("CARGO_BIN_EXE_meada")).configure(|c| {
            c.arg("mcp").current_dir(&cwd);
        }),
    )
    .unwrap();
    let uri = format!("file://{}", root.display().to_string().replace(' ', "%20"));
    let client = connect(Roots(uri, LEGACY), transport).await;

    let answer = client
        .call_tool(
            CallToolRequestParams::new("trace").with_arguments(
                serde_json::json!({"from": "sum", "direction": "down"})
                    .as_object()
                    .unwrap()
                    .clone(),
            ),
        )
        .await
        .unwrap();
    let text = answer.content.first().and_then(|c| c.as_text()).unwrap();
    assert!(
        text.text
            .starts_with("example.com/acme/root src/math.ts:5 sum\n"),
        "{}",
        text.text
    );
    client.cancel().await.unwrap();
}

#[tokio::test]
async fn a_silent_client_gets_a_tool_error_naming_roots_list() {
    let cwd = fixture_repo(
        "mcp_roots_silent",
        "ts-basic",
        Some("https://example.com/acme/cwd.git"),
    );
    let (client, mut stderr) = stops_client(&cwd, LEGACY, String::new(), 0, false).await;

    let answer = trace_sum(&client).await;
    assert_eq!(answer.is_error, Some(true), "{answer:?}");
    let text = &answer
        .content
        .first()
        .and_then(|c| c.as_text())
        .unwrap()
        .text;
    assert!(text.contains("roots/list"), "{text}");
    client.cancel().await.unwrap();
    let stderr = read_all(&mut stderr).await;
    assert!(
        stderr.lines().count() == 1 && stderr.contains("roots/list"),
        "{stderr:?}"
    );
}

#[tokio::test]
async fn a_client_that_goes_quiet_keeps_its_last_root() {
    let cwd = fixture_repo(
        "mcp_roots_once_cwd",
        "ts-basic",
        Some("https://example.com/acme/cwd.git"),
    );
    let root = fixture_repo(
        "mcp roots once root",
        "ts-basic",
        Some("https://example.com/acme/root.git"),
    );
    let uri = format!("file://{}", root.display().to_string().replace(' ', "%20"));
    let (client, mut stderr) = stops_client(&cwd, LEGACY, uri, 1, false).await;

    for _ in 0..2 {
        let answer = trace_sum(&client).await;
        let text = &answer
            .content
            .first()
            .and_then(|c| c.as_text())
            .unwrap()
            .text;
        assert!(
            text.starts_with("example.com/acme/root src/math.ts:5 sum\n"),
            "{text}"
        );
    }
    client.cancel().await.unwrap();
    let stderr = read_all(&mut stderr).await;
    assert!(
        stderr.lines().count() == 1 && stderr.contains("roots/list"),
        "{stderr:?}"
    );
}

#[tokio::test]
async fn a_failing_client_gets_a_tool_error_naming_roots_list() {
    let cwd = fixture_repo(
        "mcp_roots_fails",
        "ts-basic",
        Some("https://example.com/acme/cwd.git"),
    );
    let (client, mut stderr) = stops_client(&cwd, LEGACY, String::new(), 0, true).await;

    let answer = trace_sum(&client).await;
    assert_eq!(answer.is_error, Some(true), "{answer:?}");
    let text = &answer
        .content
        .first()
        .and_then(|c| c.as_text())
        .unwrap()
        .text;
    assert!(text.contains("roots/list"), "{text}");
    client.cancel().await.unwrap();
    let stderr = read_all(&mut stderr).await;
    assert!(
        stderr.lines().count() == 1 && stderr.contains("roots/list"),
        "{stderr:?}"
    );
}

#[tokio::test]
async fn a_client_that_fails_keeps_its_last_root() {
    let cwd = fixture_repo(
        "mcp_roots_fails_cwd",
        "ts-basic",
        Some("https://example.com/acme/cwd.git"),
    );
    let root = fixture_repo(
        "mcp roots fails root",
        "ts-basic",
        Some("https://example.com/acme/root.git"),
    );
    let uri = format!("file://{}", root.display().to_string().replace(' ', "%20"));
    let (client, mut stderr) = stops_client(&cwd, LEGACY, uri, 1, true).await;

    for _ in 0..2 {
        let answer = trace_sum(&client).await;
        let text = &answer
            .content
            .first()
            .and_then(|c| c.as_text())
            .unwrap()
            .text;
        assert!(
            text.starts_with("example.com/acme/root src/math.ts:5 sum\n"),
            "{text}"
        );
    }
    client.cancel().await.unwrap();
    let stderr = read_all(&mut stderr).await;
    assert!(
        stderr.lines().count() == 1 && stderr.contains("roots/list"),
        "{stderr:?}"
    );
}

#[tokio::test]
async fn an_empty_root_list_means_the_servers_directory() {
    let cwd = fixture_repo(
        "mcp_roots_empty",
        "ts-basic",
        Some("https://example.com/acme/cwd.git"),
    );
    let transport = TokioChildProcess::new(
        tokio::process::Command::new(env!("CARGO_BIN_EXE_meada")).configure(|c| {
            c.arg("mcp").current_dir(&cwd);
        }),
    )
    .unwrap();
    let client = connect(NoRoots(LEGACY), transport).await;

    let answer = trace_sum(&client).await;
    let text = &answer
        .content
        .first()
        .and_then(|c| c.as_text())
        .unwrap()
        .text;
    assert!(
        text.starts_with("example.com/acme/cwd src/math.ts:5 sum\n"),
        "{text}"
    );
    client.cancel().await.unwrap();
}

async fn stops_client(
    cwd: &std::path::Path,
    version: ProtocolVersion,
    root: String,
    answers: usize,
    fails: bool,
) -> (
    RunningService<RoleClient, Stops>,
    tokio::process::ChildStderr,
) {
    let (transport, stderr) = TokioChildProcess::builder(
        tokio::process::Command::new(env!("CARGO_BIN_EXE_meada")).configure(|c| {
            c.arg("mcp").current_dir(cwd);
        }),
    )
    .stderr(std::process::Stdio::piped())
    .spawn()
    .unwrap();
    let stops = Stops {
        version,
        root,
        answers: AtomicUsize::new(answers),
        fails,
    };
    (connect(stops, transport).await, stderr.unwrap())
}

async fn trace_sum<H: ClientHandler>(client: &RunningService<RoleClient, H>) -> CallToolResult {
    tokio::time::timeout(
        Duration::from_secs(5 + 15),
        client.call_tool(trace_params()),
    )
    .await
    .expect("the call hung")
    .unwrap()
}

fn trace_params() -> CallToolRequestParams {
    CallToolRequestParams::new("trace").with_arguments(
        serde_json::json!({"from": "sum", "direction": "down"})
            .as_object()
            .unwrap()
            .clone(),
    )
}

fn text_of(answer: &CallToolResult) -> &str {
    &answer
        .content
        .first()
        .and_then(|c| c.as_text())
        .unwrap()
        .text
}

fn file_uri(dir: &std::path::Path) -> String {
    format!("file://{}", dir.display().to_string().replace(' ', "%20"))
}

async fn trace_once<H: ClientHandler>(
    client: &RunningService<RoleClient, H>,
    roots: Option<serde_json::Value>,
) -> CallToolResponse {
    let mut params = trace_params();
    params.input_responses = roots.map(|r| [("roots".to_string(), r)].into());
    tokio::time::timeout(Duration::from_secs(15), client.call_tool_once(params))
        .await
        .expect("the call hung")
        .unwrap()
}

async fn read_all(stderr: &mut tokio::process::ChildStderr) -> String {
    let mut out = String::new();
    let read = tokio::io::AsyncReadExt::read_to_string(stderr, &mut out);
    tokio::time::timeout(Duration::from_secs(15), read)
        .await
        .expect("the server kept stderr open")
        .unwrap();
    out
}

struct Roots(String, ProtocolVersion);

#[expect(
    deprecated,
    reason = "roots are deprecated by SEP-2577; still how Claude Code gives the session's directory"
)]
impl ClientHandler for Roots {
    fn get_info(&self) -> ClientConfig {
        roots_config(self.1.clone())
    }

    async fn list_roots(
        &self,
        _: RequestContext<RoleClient>,
    ) -> Result<ListRootsResult, ErrorData> {
        Ok(ListRootsResult::new(vec![Root::new(self.0.clone())]))
    }
}

struct NoRoots(ProtocolVersion);

impl ClientHandler for NoRoots {
    fn get_info(&self) -> ClientConfig {
        roots_config(self.0.clone())
    }
}

struct Stops {
    version: ProtocolVersion,
    root: String,
    answers: AtomicUsize,
    fails: bool,
}

#[expect(
    deprecated,
    reason = "roots are deprecated by SEP-2577; still how Claude Code gives the session's directory"
)]
impl ClientHandler for Stops {
    fn get_info(&self) -> ClientConfig {
        roots_config(self.version.clone())
    }

    async fn list_roots(
        &self,
        _: RequestContext<RoleClient>,
    ) -> Result<ListRootsResult, ErrorData> {
        let left = self
            .answers
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |n| n.checked_sub(1));
        if left.is_err() {
            if self.fails {
                return Err(ErrorData::internal_error("roots unavailable", None));
            }
            std::future::pending::<()>().await;
        }
        Ok(ListRootsResult::new(vec![Root::new(self.root.clone())]))
    }
}

#[tokio::test]
async fn a_modern_client_answers_about_its_first_root_through_the_round_trip() {
    let cwd = fixture_repo(
        "mcp_modern_cwd",
        "ts-basic",
        Some("https://example.com/acme/cwd.git"),
    );
    let root = fixture_repo(
        "mcp modern root",
        "ts-basic",
        Some("https://example.com/acme/root.git"),
    );
    let (client, mut stderr) = stops_client(&cwd, MODERN, file_uri(&root), 2, false).await;

    let first = trace_once(&client, None).await;
    let CallToolResponse::InputRequired(asked) = first else {
        panic!("the server answered without asking for the roots: {first:?}");
    };
    let asked = serde_json::to_value(asked).unwrap();
    assert_eq!(asked["inputRequests"]["roots"]["method"], "roots/list");

    let answer = trace_sum(&client).await;
    assert!(
        text_of(&answer).starts_with("example.com/acme/root src/math.ts:5 sum\n"),
        "{}",
        text_of(&answer)
    );
    client.cancel().await.unwrap();
    assert_eq!(read_all(&mut stderr).await, "");
}

#[tokio::test]
async fn a_modern_empty_root_list_means_the_servers_directory() {
    let cwd = fixture_repo(
        "mcp_modern_empty",
        "ts-basic",
        Some("https://example.com/acme/cwd.git"),
    );
    let (transport, _stderr) = server_in(&cwd);
    let client = connect(NoRoots(MODERN), transport).await;

    let answer = trace_sum(&client).await;
    assert!(
        text_of(&answer).starts_with("example.com/acme/cwd src/math.ts:5 sum\n"),
        "{}",
        text_of(&answer)
    );
    client.cancel().await.unwrap();
}

#[tokio::test]
async fn a_modern_retry_without_a_usable_root_gets_a_tool_error_naming_roots_list() {
    let cwd = fixture_repo(
        "mcp_modern_fails",
        "ts-basic",
        Some("https://example.com/acme/cwd.git"),
    );
    let (client, mut stderr) = stops_client(&cwd, MODERN, String::new(), 0, true).await;

    for roots in [
        serde_json::json!({"roots": "not a list"}),
        serde_json::json!({"elsewhere": {}}),
    ] {
        let CallToolResponse::Complete(answer) = (match roots.get("roots") {
            Some(bad) => trace_once(&client, Some(bad.clone())).await,
            None => {
                let mut params = trace_params();
                params.input_responses = Some([("elsewhere".to_string(), roots)].into());
                client.call_tool_once(params).await.unwrap()
            }
        }) else {
            panic!("the server asked again");
        };
        assert_eq!(answer.is_error, Some(true), "{answer:?}");
        assert!(
            text_of(&answer).contains("roots/list"),
            "{}",
            text_of(&answer)
        );
    }
    client.cancel().await.unwrap();
    let stderr = read_all(&mut stderr).await;
    assert!(
        stderr.lines().count() == 2 && stderr.lines().all(|l| l.contains("roots/list")),
        "{stderr:?}"
    );
}

#[tokio::test]
async fn a_modern_client_that_fails_keeps_its_last_root() {
    let cwd = fixture_repo(
        "mcp_modern_fails_cwd",
        "ts-basic",
        Some("https://example.com/acme/cwd.git"),
    );
    let root = fixture_repo(
        "mcp modern fails root",
        "ts-basic",
        Some("https://example.com/acme/root.git"),
    );
    let (client, mut stderr) = stops_client(&cwd, MODERN, file_uri(&root), 1, true).await;

    let answer = trace_sum(&client).await;
    assert!(text_of(&answer).starts_with("example.com/acme/root src/math.ts:5 sum\n"));
    let CallToolResponse::Complete(answer) =
        trace_once(&client, Some(serde_json::json!({"roots": "not a list"}))).await
    else {
        panic!("the server asked again");
    };
    assert!(
        text_of(&answer).starts_with("example.com/acme/root src/math.ts:5 sum\n"),
        "{}",
        text_of(&answer)
    );
    client.cancel().await.unwrap();
    let stderr = read_all(&mut stderr).await;
    assert!(
        stderr.lines().count() == 1 && stderr.contains("roots/list"),
        "{stderr:?}"
    );
}

#[tokio::test]
async fn a_client_without_roots_gets_the_servers_directory() {
    let cwd = fixture_repo(
        "mcp_no_capability",
        "ts-basic",
        Some("https://example.com/acme/cwd.git"),
    );
    let (transport, _stderr) = server_in(&cwd);
    let mut config = ClientConfig::default();
    config.protocol_version = MODERN;
    let client = connect(config, transport).await;

    let CallToolResponse::Complete(answer) = trace_once(&client, None).await else {
        panic!("the server asked for roots the client did not declare");
    };
    assert!(
        text_of(&answer).starts_with("example.com/acme/cwd src/math.ts:5 sum\n"),
        "{}",
        text_of(&answer)
    );
    client.cancel().await.unwrap();
}

fn server_in(cwd: &std::path::Path) -> (TokioChildProcess, Option<tokio::process::ChildStderr>) {
    TokioChildProcess::builder(
        tokio::process::Command::new(env!("CARGO_BIN_EXE_meada")).configure(|c| {
            c.arg("mcp").current_dir(cwd);
        }),
    )
    .stderr(std::process::Stdio::piped())
    .spawn()
    .unwrap()
}
