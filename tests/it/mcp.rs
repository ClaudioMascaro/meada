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

/// The eras the server tells apart: before 2026-07-28 it sends `roots/list`; from there on it
/// answers the call with `InputRequiredResult` (SEP-2322).
const LEGACY: ProtocolVersion = ProtocolVersion::V_2025_11_25;
const MODERN: ProtocolVersion = ProtocolVersion::V_2026_07_28;

/// Serves `handler` in the era its config names: the `initialize` handshake before 2026-07-28
/// (rmcp's `serve` always does that, whatever version the config says), `server/discover` from
/// there on.
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

/// A config declaring the roots capability, speaking `version`.
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

/// `meada mcp` in a fixture repository: the tools it lists stay within 1,000 tokens (one per
/// four bytes of description and parameter schema, ADR 0008), the list matches its golden
/// (names, parameter schemas and descriptions: the surface 1.0 freezes), and `trace` answers.
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
    assert_eq!(
        text.text,
        "example.com/acme/web src/math.ts:5 sum\n  example.com/acme/web src/math.ts:1 add\n\
         chain: exact · 0 hops · 1 repos · 32 tokens\n"
    );
    client.cancel().await.unwrap();
}

/// The client's first `file://` root wins over the server's cwd (#192): a client that answers
/// `roots/list` with another repository, under a directory with a space in its name, gets
/// answers about that repository.
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

/// A client that stops answering `roots/list` (#282) must not hang the call: the server waits
/// 5 s, then returns a tool error that names `roots/list` (no root was ever sent, and the
/// server's own directory could be another checkout), with one line on stderr.
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

/// A client that answers `roots/list` once and then goes quiet (#282) keeps its last root: the
/// call that times out answers about it, not about the server's cwd, with one line on stderr.
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

/// A client that fails `roots/list` with a JSON-RPC error (#285) is handled like one that goes
/// quiet: with no root sent before, the call returns a tool error naming `roots/list`, with one
/// line on stderr.
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

/// A client that answers `roots/list` once and then fails it (#285) keeps its last root: the
/// failing call answers about it, not about the server's cwd, with one line on stderr.
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

/// A client that declares roots but answers `roots/list` with an empty list (#285) is a
/// successful answer with no local root: it means the server's directory, as without the
/// capability, not an error.
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

/// Serves `meada mcp` in `cwd` to a [`Stops`] client, keeping the server's stderr.
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

/// `trace` from `sum`, down. The server waits 5 s for `roots/list`; the 15 s of slack on top
/// make a hang fail the test instead of stalling it.
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

/// The first text block of a result.
fn text_of(answer: &CallToolResult) -> &str {
    &answer
        .content
        .first()
        .and_then(|c| c.as_text())
        .unwrap()
        .text
}

/// The `file://` URI of a directory.
fn file_uri(dir: &std::path::Path) -> String {
    format!("file://{}", dir.display().to_string().replace(' ', "%20"))
}

/// One `tools/call` of `trace`, answered or not, with the retry's `roots` answer (if any).
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

/// The server's stderr up to its exit; a server that never closes it fails the test.
async fn read_all(stderr: &mut tokio::process::ChildStderr) -> String {
    let mut out = String::new();
    let read = tokio::io::AsyncReadExt::read_to_string(stderr, &mut out);
    tokio::time::timeout(Duration::from_secs(15), read)
        .await
        .expect("the server kept stderr open")
        .unwrap();
    out
}

/// A client of `version` that declares the roots capability and answers `roots/list` with one
/// `file://` root.
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

/// A client of `version` that declares the roots capability and answers `roots/list` with an
/// empty list, the default of rmcp's `list_roots`.
struct NoRoots(ProtocolVersion);

impl ClientHandler for NoRoots {
    fn get_info(&self) -> ClientConfig {
        roots_config(self.0.clone())
    }
}

/// A client of `version` that declares the roots capability, answers `roots/list` with `root`
/// while it has `answers` left, then never answers again, or, with `fails`, answers with an
/// error.
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

/// On the 2026-07-28 era the client drops `roots/list` requests: the first call is answered
/// with `InputRequiredResult` asking for `roots` (SEP-2322), and rmcp's `call_tool` fulfils it
/// through `list_roots` and retries. The answer is about the client's first root, not the cwd.
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

    // The first call by hand: the server asks for the roots instead of answering.
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

/// A modern client that answers with an empty list gets the server's directory, as on the
/// legacy era.
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

/// rmcp's `call_tool` aborts when `list_roots` fails, so the failing retries are built by hand:
/// a `roots` answer that is missing or malformed, with no root sent before, is a tool error
/// naming `roots/list`, with one line on stderr.
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
            // An `inputResponses` that lacks `roots`.
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

/// A modern client that answered once and then sends a malformed `roots` keeps its last root:
/// the failing call answers about it, with one line on stderr.
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

/// A client that does not declare roots gets the server's directory, with no round trip. The
/// capability check comes before the era check, so one era (modern) covers both.
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

/// `meada mcp` in `cwd`, with its stderr piped.
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
