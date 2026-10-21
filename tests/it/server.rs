//! Server mode's HTTP server (spec #259, ADR 0011): `meada mcp --listen <addr> --workspace
//! <manifest>` as a child process over the chain's mirrors, driven by rmcp's streamable HTTP
//! client as an agent drives it, and by plain HTTP requests where no MCP client goes (the
//! 404, calls with no session to two servers).

use crate::common;

use std::fs;
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::process::{Child, ChildStderr, Command, Stdio};

use crate::manifest::{
    MANIFEST, ReadOnly, assert_nothing_else_written, chain_manifest, contents, run, run_export,
};
use common::{assert_golden, chain};
use rmcp::{
    RoleClient, ServiceExt,
    model::{CallToolRequestParams, CallToolResult, Tool},
    service::RunningService,
    transport::{ConfigureCommandExt, StreamableHttpClientTransport, TokioChildProcess},
};

const WEB: &str = "example.com/chain/web";

/// The chain's identities in manifest order, as the 404 lists them.
const SERVED: &str = "example.com/chain/web\nexample.com/chain/gateway\nexample.com/chain/catalog\n\
                      example.com/chain/worker\n";

/// `meada mcp --listen <listen> --workspace ops/meada.workspace.toml` running in `dir`, killed
/// when dropped. `url` is the address its first stderr line names.
struct Server {
    child: Child,
    url: String,
    // Kept open, so a later line on stderr does not fail on a closed pipe.
    _stderr: BufReader<ChildStderr>,
}

impl Server {
    fn start(dir: &Path, listen: &str) -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_meada"))
            .current_dir(dir)
            .args(["mcp", "--listen", listen, "--workspace", MANIFEST])
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .expect("meada runs");
        let mut stderr = BufReader::new(child.stderr.take().unwrap());
        let mut line = String::new();
        stderr.read_line(&mut line).unwrap();
        let url = (line.strip_prefix("listening on "))
            .unwrap_or_else(|| panic!("first line: {line:?}"))
            .trim_end()
            .to_string();
        Self {
            child,
            url,
            _stderr: stderr,
        }
    }

    /// An MCP client of the endpoint at `path`.
    async fn client(&self, path: &str) -> RunningService<RoleClient, ()> {
        let transport = StreamableHttpClientTransport::from_uri(format!("{}{path}", self.url));
        ().serve(transport).await.unwrap()
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// `name` called with `arguments`.
fn call(name: &'static str, arguments: serde_json::Value) -> CallToolRequestParams {
    CallToolRequestParams::new(name).with_arguments(arguments.as_object().unwrap().clone())
}

/// A tool result's text.
fn text(result: &CallToolResult) -> &str {
    &result
        .content
        .first()
        .and_then(|c| c.as_text())
        .unwrap()
        .text
}

/// The tool list as the budget counts it: one token per four bytes of description and
/// parameter schema (ADR 0008).
fn tokens(tools: &[Tool]) -> usize {
    let bytes: usize = (tools.iter())
        .map(|t| {
            t.description.as_deref().unwrap_or("").len()
                + serde_json::to_string(&*t.input_schema).unwrap().len()
        })
        .sum();
    bytes.div_ceil(4)
}

/// An HTTP agent that reads 4xx and 5xx answers instead of failing on them.
fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .http_status_as_error(false)
        .build()
        .new_agent()
}

/// `GET url`: status, `Content-Type` and body.
fn get(url: &str) -> (u16, String, String) {
    let mut response = agent().get(url).call().unwrap();
    let kind = response.headers()["content-type"]
        .to_str()
        .unwrap()
        .to_string();
    let body = response.body_mut().read_to_string().unwrap();
    (response.status().as_u16(), kind, body)
}

/// One JSON-RPC message POSTed to `url` alone, as a client with no session sends it: the
/// status, whether a session id came back, the `Content-Type` and the body as JSON (a string
/// when it is not JSON).
fn post(url: &str, message: serde_json::Value) -> (u16, bool, String, serde_json::Value) {
    post_with(url, message, &[])
}

/// `post` with more `headers`.
fn post_with(
    url: &str,
    message: serde_json::Value,
    headers: &[(&str, &str)],
) -> (u16, bool, String, serde_json::Value) {
    let mut request = agent().post(url);
    for (name, value) in headers {
        request = request.header(*name, *value);
    }
    let mut response = request
        .header("Accept", "application/json, text/event-stream")
        .header("Content-Type", "application/json")
        .send(message.to_string())
        .unwrap();
    let session = response.headers().contains_key("mcp-session-id");
    let kind = (response.headers().get("content-type"))
        .map_or("", |v| v.to_str().unwrap())
        .to_string();
    let body = response.body_mut().read_to_string().unwrap();
    let json = serde_json::from_str(&body).unwrap_or(serde_json::Value::String(body));
    (response.status().as_u16(), session, kind, json)
}

/// `trace` from `dashboard` over stdio, in a chain of its own: the answer server mode matches.
async fn stdio_trace(case: &str) -> (Vec<Tool>, String) {
    let (web, _, _) = chain(case);
    let transport = TokioChildProcess::new(
        tokio::process::Command::new(env!("CARGO_BIN_EXE_meada")).configure(|c| {
            c.arg("mcp").current_dir(&web);
        }),
    )
    .unwrap();
    let client = ().serve(transport).await.unwrap();
    let tools = client.list_all_tools().await.unwrap();
    let answer = (client.call_tool(call("trace", serde_json::json!({"from": "dashboard"}))))
        .await
        .unwrap();
    client.cancel().await.unwrap();
    (tools, text(&answer).to_string())
}

/// The HTTP smoke test: the stdio server's tools within the budget, the stdio `trace` byte for
/// byte, the 404 with the identities served, the `uncommitted` refusal, the behind line before
/// and after `meada index`, an index problem at query time, and nothing written by a call.
#[tokio::test]
async fn serves_each_repository_over_http_as_the_stdio_server_answers_it() {
    let dir = chain_manifest("server_http");
    assert!(run(&dir, "index").2);
    let indexes = dir.join("indexes");
    let indexed = contents(&indexes);
    let server = Server::start(&dir, "127.0.0.1:0");
    assert!(
        server.url.starts_with("http://127.0.0.1:"),
        "{}",
        server.url
    );
    let client = server.client(&format!("/{WEB}")).await;

    let info = client.peer_info().unwrap();
    assert!(
        info.instructions.as_deref().unwrap().contains(WEB),
        "{info:?}"
    );
    let tools = client.list_all_tools().await.unwrap();
    let mut names: Vec<&str> = tools.iter().map(|t| &*t.name).collect();
    names.sort();
    assert_eq!(names, ["context", "search", "trace"]);
    assert!(
        tokens(&tools) <= 1_000,
        "tool list costs {} tokens",
        tokens(&tools)
    );

    let trace = (client.call_tool(call("trace", serde_json::json!({"from": "dashboard"}))))
        .await
        .unwrap();
    let (stdio_tools, stdio_trace) = stdio_trace("server_http_stdio").await;
    assert_eq!(text(&trace), stdio_trace);
    assert_golden("chain.trace-down.txt", text(&trace));
    // The same definitions: names, descriptions and parameter schemas.
    let shape = |tools: &[Tool]| {
        let mut shape: Vec<String> = (tools.iter())
            .map(|t| serde_json::json!([t.name, t.description, t.input_schema]).to_string())
            .collect();
        shape.sort();
        shape
    };
    assert_eq!(shape(&tools), shape(&stdio_tools));

    // A trailing slash names the same repository.
    let slash = server.client(&format!("/{WEB}/")).await;
    let answer = (slash.call_tool(call("trace", serde_json::json!({"from": "dashboard"}))))
        .await
        .unwrap();
    assert_eq!(text(&answer), stdio_trace);
    slash.cancel().await.unwrap();

    // No default repository: the root and an unlisted identity list the ones served.
    for path in ["", "/", "/example.com/chain/nope", "/example.com/chain"] {
        let (status, kind, body) = get(&format!("{}{path}", server.url));
        assert_eq!((status, body.as_str()), (404, SERVED), "{path}");
        assert!(kind.starts_with("text/plain"), "{kind}");
    }

    let uncommitted = (client
        .call_tool(call("trace", serde_json::json!({"changes": "uncommitted"}))))
    .await
    .unwrap();
    assert_eq!(uncommitted.is_error, Some(true));
    assert_eq!(
        text(&uncommitted),
        "server mode has no working tree to diff: run Meada locally for `changes: uncommitted`"
    );

    let context = || client.call_tool(call("context", serde_json::json!({"symbol": "dashboard"})));
    let fresh = context().await.unwrap();
    let fresh = text(&fresh).to_string();
    assert!(fresh.contains("async function dashboard() {"), "{fresh}");
    assert!(contents(&indexes) == indexed, "a call wrote an index");

    // The mirror moves and the server, which refreshes nothing, says the index is behind.
    let file = dir.join("chain-web/src/dashboard.ts");
    let source = fs::read_to_string(&file).unwrap();
    fs::write(&file, format!("{source}\n// moved\n")).unwrap();
    let behind = context().await.unwrap();
    let behind = text(&behind).to_string();
    let mut lines = behind.lines();
    assert_eq!(
        lines.next(),
        Some("example.com/chain/web src/dashboard.ts:4 dashboard")
    );
    assert!(!behind.contains("async function"), "{behind}");
    assert_eq!(
        (behind.lines())
            .filter(|l| *l == "index behind the mirror: run meada index --workspace")
            .count(),
        1,
        "{behind}"
    );
    assert!(contents(&indexes) == indexed, "a call wrote an index");
    assert!(run(&dir, "index").2);
    assert_eq!(text(&context().await.unwrap()), fresh);

    // At query time the named repository's mirror alone is checked again: a mirror that is no
    // checkout any more refuses its own endpoint, and the others answer.
    let worker = dir.join("chain-worker");
    let root = worker.canonicalize().unwrap();
    fs::rename(worker.join(".git"), worker.join("moved.git")).unwrap();
    let broken = server.client("/example.com/chain/worker").await;
    let refused = (broken.call_tool(call("search", serde_json::json!({"query": "dashboard"}))))
        .await
        .unwrap();
    assert_eq!(refused.is_error, Some(true));
    assert_eq!(
        text(&refused),
        format!(
            "example.com/chain/worker: `{}` is not the root of a checkout with a working tree",
            root.display()
        )
    );
    broken.cancel().await.unwrap();
    let search = (client.call_tool(call("search", serde_json::json!({"query": "dashboard"}))))
        .await
        .unwrap();
    assert_ne!(search.is_error, Some(true), "{}", text(&search));
    fs::rename(worker.join("moved.git"), worker.join(".git")).unwrap();

    // At query time an index the server cannot read is a tool error naming the fix.
    let catalog = indexes.join("example.com/chain/catalog/index.db");
    fs::write(&catalog, "not an index\n").unwrap();
    let search = (client.call_tool(call("search", serde_json::json!({"query": "dashboard"}))))
        .await
        .unwrap();
    assert_eq!(search.is_error, Some(true));
    assert_eq!(
        text(&search),
        "example.com/chain/catalog: indexed by another version; run meada index --workspace \
         ops/meada.workspace.toml"
    );
    assert_eq!(fs::read_to_string(&catalog).unwrap(), "not an index\n");

    client.cancel().await.unwrap();
    let all: Vec<&str> = crate::manifest::CHAIN.iter().map(|(r, _)| *r).collect();
    assert_nothing_else_written(&dir, &all);
}

/// A node that receives a built index mounts `index_dir` and the mirrors read-only (spec #259):
/// the server answers `trace` as the golden has it, `status` and `export` answer as on a
/// writable layout, and nothing is created anywhere.
#[cfg(unix)]
#[tokio::test]
async fn serves_from_read_only_indexes_and_mirrors_and_creates_nothing() {
    let dir = chain_manifest("server_read_only");
    assert!(run(&dir, "index").2);
    let (status, _, _) = run(&dir, "status");
    let (export, _, _) = run_export(&dir, WEB);
    let before = contents(&dir);

    let Some(_locked) = ReadOnly::lock(&dir) else {
        return;
    };
    let (locked_status, stderr, ok) = run(&dir, "status");
    assert!(ok && stderr.is_empty(), "{stderr}");
    assert_eq!(locked_status, status);
    let (locked_export, stderr, code) = run_export(&dir, WEB);
    assert_eq!((code, stderr.as_str()), (Some(0), ""));
    assert_eq!(locked_export, export);

    let server = Server::start(&dir, "127.0.0.1:0");
    let client = server.client(&format!("/{WEB}")).await;
    let trace = (client.call_tool(call("trace", serde_json::json!({"from": "dashboard"}))))
        .await
        .unwrap();
    assert_golden("chain.trace-down.txt", text(&trace));
    let context = (client.call_tool(call("context", serde_json::json!({"symbol": "dashboard"}))))
        .await
        .unwrap();
    assert!(text(&context).contains("async function dashboard() {"));
    client.cancel().await.unwrap();

    assert!(contents(&dir) == before, "something was created or changed");
}

/// Stateless with JSON responses: no session comes back, and calls sent alternately to two
/// servers over one manifest, none of them initialized there, are answered alike. `:0` with
/// no host binds the loopback interface.
#[test]
fn two_servers_over_one_manifest_answer_alternate_calls_with_no_session() {
    let dir = chain_manifest("server_two");
    assert!(run(&dir, "index").2);
    let one = Server::start(&dir, "127.0.0.1:0");
    let two = Server::start(&dir, ":0");
    assert!(two.url.starts_with("http://127.0.0.1:"), "{}", two.url);

    let initialize = serde_json::json!({
        "jsonrpc": "2.0", "id": 0, "method": "initialize",
        "params": {
            "protocolVersion": "2025-06-18", "capabilities": {},
            "clientInfo": {"name": "test", "version": "0"}
        }
    });
    let (status, session, kind, _) = post(&format!("{}/{WEB}", one.url), initialize.clone());
    assert_eq!((status, session), (200, false));
    assert!(kind.starts_with("application/json"), "{kind}");
    // A reverse proxy forwards its own hostname; a browser's page (DNS rebinding) sends an
    // `Origin`, which no MCP client does.
    let url = format!("{}/{WEB}", one.url);
    let proxied = post_with(&url, initialize.clone(), &[("Host", "meada.internal")]);
    assert_eq!(proxied.0, 200, "{proxied:?}");
    let browser = post_with(
        &url,
        initialize.clone(),
        &[("Origin", "http://meada.internal")],
    );
    assert_eq!(browser.0, 403, "{browser:?}");
    // The path is percent-decoded before it names a repository.
    let encoded = post(
        &format!("{}/example.com%2Fchain%2Fweb", one.url),
        initialize,
    );
    assert_eq!(encoded.0, 200, "{encoded:?}");

    let golden = fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/chain.trace-down.txt"),
    )
    .unwrap();
    for (n, server) in [&two, &one, &two, &one].into_iter().enumerate() {
        let message = serde_json::json!({
            "jsonrpc": "2.0", "id": n + 1, "method": "tools/call",
            "params": {"name": "trace", "arguments": {"from": "dashboard"}}
        });
        let (status, session, kind, json) = post(&format!("{}/{WEB}", server.url), message);
        assert_eq!((status, session), (200, false), "{json}");
        assert!(kind.starts_with("application/json"), "{kind}");
        assert_eq!(
            json["result"]["content"][0]["text"],
            golden.as_str(),
            "{json}"
        );
    }
}

/// Before `meada index`, every repository's missing index refuses the start, each named with
/// the fix; and `--listen` and `--workspace` go together.
#[test]
fn refuses_to_start_listing_every_repository_that_cannot_be_served() {
    let dir = chain_manifest("server_refuse");
    let start = |args: &[&str]| {
        let out = Command::new(env!("CARGO_BIN_EXE_meada"))
            .current_dir(&dir)
            .args(args)
            .output()
            .expect("meada runs");
        (String::from_utf8(out.stderr).unwrap(), out.status.code())
    };
    // Nothing listens while the checks fail: the port is free before and after.
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let at = format!("127.0.0.1:{port}");
    let (stderr, code) = start(&["mcp", "--listen", &at, "--workspace", MANIFEST]);
    assert_eq!(code, Some(1));
    assert!(std::net::TcpStream::connect(&at).is_err());
    let fix = "not indexed; run meada index --workspace ops/meada.workspace.toml";
    assert_eq!(
        stderr,
        format!(
            "meada: example.com/chain/web: {fix}\nexample.com/chain/gateway: {fix}\n\
             example.com/chain/catalog: {fix}\nexample.com/chain/worker: {fix}\n"
        )
    );
    assert!(!dir.join("indexes").exists());

    let (stderr, code) = start(&["mcp", "--listen", ":0"]);
    assert!(
        code == Some(2) && stderr.contains("--workspace"),
        "{stderr}"
    );
    let (stderr, code) = start(&["mcp", "--workspace", MANIFEST]);
    assert!(code == Some(2) && stderr.contains("--listen"), "{stderr}");
}

/// SIGTERM after a completed call: the server stops and exits 0 (graceful stop, spec #259).
#[cfg(unix)]
#[tokio::test]
async fn exits_zero_on_sigterm_after_a_call() {
    let dir = chain_manifest("server_sigterm");
    assert!(run(&dir, "index").2);
    let mut server = Server::start(&dir, "127.0.0.1:0");
    let client = server.client(&format!("/{WEB}")).await;
    let result = client
        .call_tool(call("trace", serde_json::json!({"from": "dashboard"})))
        .await
        .unwrap();
    assert!(!text(&result).is_empty());
    // The client keeps its connection open: the stop does not wait for it.
    // (A SIGTERM sent right after the address, with no call, is the next test.)
    let kill = Command::new("kill")
        .args(["-TERM", &server.child.id().to_string()])
        .status()
        .unwrap();
    assert!(kill.success());
    assert_eq!(server.child.wait().unwrap().code(), Some(0));
}

/// SIGTERM sent the moment the address is read, before any call: still a clean exit 0, since
/// the handlers are installed before the address is printed.
#[cfg(unix)]
#[test]
fn exits_zero_on_sigterm_right_after_the_listening_line() {
    let dir = chain_manifest("server_sigterm_early");
    assert!(run(&dir, "index").2);
    for _ in 0..20 {
        let mut server = Server::start(&dir, "127.0.0.1:0");
        let kill = Command::new("kill")
            .args(["-TERM", &server.child.id().to_string()])
            .status()
            .unwrap();
        assert!(kill.success());
        assert_eq!(server.child.wait().unwrap().code(), Some(0));
    }
}
