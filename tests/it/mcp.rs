use crate::common;

use std::{
    sync::atomic::{AtomicUsize, Ordering},
    time::Duration,
};

use common::fixture_repo;
use rmcp::{
    ClientHandler, ServiceExt,
    model::{
        CallToolRequestParams, CallToolResult, ClientCapabilities, ClientConfig, ErrorData,
        Implementation,
    },
    service::{RequestContext, RoleClient, RunningService},
    transport::{ConfigureCommandExt, TokioChildProcess},
};

#[expect(
    deprecated,
    reason = "roots are deprecated by SEP-2577; the server still follows them (#192)"
)]
use rmcp::model::{ListRootsResult, Root};

/// `meada mcp` in a fixture repository: the tools it lists stay within 1,000 tokens (one per
/// four bytes of description and parameter schema, ADR 0008), and `trace` answers.
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
    common::assert_golden(
        "surface.mcp-tools.json",
        &format!("{}\n", serde_json::to_string_pretty(&tools).unwrap()),
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

/// The effective default stays 2,000 for every tool, even if its schema text stays unchanged.
#[tokio::test]
async fn the_three_tools_default_to_a_2000_token_budget() {
    let repo = fixture_repo(
        "mcp_default_budget",
        "ts-basic",
        Some("https://example.com/acme/web.git"),
    );
    let mut source = String::from("export function entrypoint() {\n");
    for i in 0..256 {
        source.push_str(&format!("  step{i:03}();\n"));
    }
    source.push_str("}\n");
    for i in 0..256 {
        source.push_str(&format!("export function step{i:03}() {{ return {i}; }}\n"));
    }
    std::fs::write(repo.join("src/surface.ts"), source).unwrap();
    let transport = TokioChildProcess::new(
        tokio::process::Command::new(env!("CARGO_BIN_EXE_meada")).configure(|c| {
            c.arg("mcp").current_dir(&repo);
        }),
    )
    .unwrap();
    let client = ().serve(transport).await.unwrap();
    for (tool, args) in [
        ("search", serde_json::json!({"query": ""})),
        ("context", serde_json::json!({"symbol": "entrypoint"})),
        ("trace", serde_json::json!({"from": "entrypoint"})),
    ] {
        let mut answers = Vec::new();
        for budget in [None, Some(2000)] {
            let mut args = args.as_object().unwrap().clone();
            if let Some(budget) = budget {
                args.insert("budget".into(), serde_json::json!(budget));
            }
            let answer = client
                .call_tool(CallToolRequestParams::new(tool).with_arguments(args))
                .await
                .unwrap();
            assert_ne!(answer.is_error, Some(true), "{tool}: {answer:?}");
            let text = &answer
                .content
                .first()
                .and_then(|c| c.as_text())
                .unwrap()
                .text;
            answers.push(text.clone());
        }
        assert_eq!(answers[0], answers[1], "{tool}: default budget changed");
        assert!(
            answers[0].contains("(budget 2000 tokens)"),
            "{tool}: the fixture must exceed the default budget"
        );
        assert!(answers[0].len().div_ceil(4) <= 2000, "{tool}");
    }
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
    let client = Roots(uri).serve(transport).await.unwrap();

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
    let (client, mut stderr) = stops_client(&cwd, String::new(), 0, false).await;

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
    let (client, mut stderr) = stops_client(&cwd, uri, 1, false).await;

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
    let (client, mut stderr) = stops_client(&cwd, String::new(), 0, true).await;

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
    let (client, mut stderr) = stops_client(&cwd, uri, 1, true).await;

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
    let client = NoRoots.serve(transport).await.unwrap();

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
        root,
        answers: AtomicUsize::new(answers),
        fails,
    };
    (stops.serve(transport).await.unwrap(), stderr.unwrap())
}

/// `trace` from `sum`, down. The server waits 5 s for `roots/list`; the 15 s of slack on top
/// make a hang fail the test instead of stalling it.
async fn trace_sum<H: ClientHandler>(client: &RunningService<RoleClient, H>) -> CallToolResult {
    let params = CallToolRequestParams::new("trace").with_arguments(
        serde_json::json!({"from": "sum", "direction": "down"})
            .as_object()
            .unwrap()
            .clone(),
    );
    tokio::time::timeout(Duration::from_secs(5 + 15), client.call_tool(params))
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

/// A client that declares the roots capability and answers `roots/list` with one `file://` root.
struct Roots(String);

#[expect(
    deprecated,
    reason = "roots are deprecated by SEP-2577; the server still follows them (#192)"
)]
impl ClientHandler for Roots {
    fn get_info(&self) -> ClientConfig {
        ClientConfig::new(
            ClientCapabilities::builder().enable_roots().build(),
            Implementation::from_build_env(),
        )
    }

    async fn list_roots(
        &self,
        _: RequestContext<RoleClient>,
    ) -> Result<ListRootsResult, ErrorData> {
        Ok(ListRootsResult::new(vec![Root::new(self.0.clone())]))
    }
}

/// A client that declares the roots capability and answers `roots/list` with an empty list, the
/// default of rmcp's `list_roots`.
struct NoRoots;

#[expect(
    deprecated,
    reason = "roots are deprecated by SEP-2577; the server still follows them (#285)"
)]
impl ClientHandler for NoRoots {
    fn get_info(&self) -> ClientConfig {
        ClientConfig::new(
            ClientCapabilities::builder().enable_roots().build(),
            Implementation::from_build_env(),
        )
    }
}

/// A client that declares the roots capability, answers `roots/list` with `root` while it has
/// `answers` left, then never answers again, or, with `fails`, answers with an error.
struct Stops {
    root: String,
    answers: AtomicUsize,
    fails: bool,
}

#[expect(
    deprecated,
    reason = "roots are deprecated by SEP-2577; the server still follows them (#282)"
)]
impl ClientHandler for Stops {
    fn get_info(&self) -> ClientConfig {
        ClientConfig::new(
            ClientCapabilities::builder().enable_roots().build(),
            Implementation::from_build_env(),
        )
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
