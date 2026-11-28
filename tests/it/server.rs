use crate::common;

use std::fs;
use std::io::{BufRead, BufReader};
use std::net::TcpStream;
use std::path::Path;
use std::process::{Child, ChildStderr, Command, Stdio};
use std::time::{Duration, Instant};

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

const SERVED: &str = "example.com/chain/web\nexample.com/chain/gateway\nexample.com/chain/catalog\n\
                      example.com/chain/worker\n";

pub(crate) struct Server {
    child: Child,
    pub(crate) url: String,
    stderr: BufReader<ChildStderr>,
}

impl Server {
    fn start(dir: &Path, listen: &str) -> Self {
        Self::start_with(dir, listen, &[])
    }

    fn start_with(dir: &Path, listen: &str, env: &[(&str, &str)]) -> Self {
        Self::start_args(dir, &["--listen", listen, "--workspace", MANIFEST], env)
    }

    pub(crate) fn start_args(dir: &Path, args: &[&str], env: &[(&str, &str)]) -> Self {
        Self::start_command(dir, "mcp", args, env)
    }

    pub(crate) fn start_command(
        dir: &Path,
        command: &str,
        args: &[&str],
        env: &[(&str, &str)],
    ) -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_meada"))
            .envs(env.iter().copied())
            .current_dir(dir)
            .arg(command)
            .args(args)
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
        Self { child, url, stderr }
    }

    #[cfg_attr(not(feature = "oidc"), expect(dead_code))]
    pub(crate) fn stderr(mut self) -> String {
        use std::io::Read;
        let _ = self.child.kill();
        let _ = self.child.wait();
        let mut rest = String::new();
        let _ = self.stderr.read_to_string(&mut rest);
        rest
    }

    #[cfg_attr(not(feature = "accounts"), expect(dead_code))]
    pub(crate) fn next_line(&mut self) -> String {
        let mut line = String::new();
        self.stderr.read_line(&mut line).unwrap();
        line.trim_end_matches('\n').to_string()
    }

    pub(crate) async fn client(&self, path: &str) -> RunningService<RoleClient, ()> {
        crate::common::crypto_provider();
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

#[cfg(unix)]
pub(crate) fn terminate(server: &Server) {
    let kill = Command::new("kill")
        .args(["-TERM", &server.child.id().to_string()])
        .status()
        .unwrap();
    assert!(kill.success());
}

#[cfg(unix)]
pub(crate) fn exit_within(server: &mut Server, wait: Duration) -> Option<Option<i32>> {
    let start = Instant::now();
    while start.elapsed() < wait {
        if let Some(status) = server.child.try_wait().unwrap() {
            return Some(status.code());
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    None
}

#[cfg(unix)]
async fn in_flight() {
    tokio::time::sleep(Duration::from_millis(1500)).await;
}

#[cfg(unix)]
fn commit_in_progress(dir: &Path) -> rusqlite::Connection {
    let db = rusqlite::Connection::open(dir.join("indexes").join(WEB).join("index.db")).unwrap();
    db.execute_batch("BEGIN EXCLUSIVE").unwrap();
    db
}

pub(crate) fn call(name: &'static str, arguments: serde_json::Value) -> CallToolRequestParams {
    CallToolRequestParams::new(name).with_arguments(arguments.as_object().unwrap().clone())
}

pub(crate) fn text(result: &CallToolResult) -> &str {
    &result
        .content
        .first()
        .and_then(|c| c.as_text())
        .unwrap()
        .text
}

pub(crate) fn tokens(tools: &[Tool]) -> usize {
    let bytes: usize = (tools.iter())
        .map(|t| {
            t.description.as_deref().unwrap_or("").len()
                + serde_json::to_string(&*t.input_schema).unwrap().len()
        })
        .sum();
    bytes.div_ceil(4)
}

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .http_status_as_error(false)
        .build()
        .new_agent()
}

pub(crate) fn get(url: &str) -> (u16, String, String) {
    get_with(url, &[])
}

pub(crate) fn get_with(url: &str, headers: &[(&str, &str)]) -> (u16, String, String) {
    let mut request = agent().get(url);
    for (name, value) in headers {
        request = request.header(*name, *value);
    }
    let mut response = request.call().unwrap();
    let kind = response.headers()["content-type"]
        .to_str()
        .unwrap()
        .to_string();
    let body = response.body_mut().read_to_string().unwrap();
    (response.status().as_u16(), kind, body)
}

fn post(url: &str, message: serde_json::Value) -> (u16, bool, String, serde_json::Value) {
    post_with(url, message, &[])
}

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
    let shape = |tools: &[Tool]| {
        let mut shape: Vec<String> = (tools.iter())
            .map(|t| serde_json::json!([t.name, t.description, t.input_schema]).to_string())
            .collect();
        shape.sort();
        shape
    };
    assert_eq!(shape(&tools), shape(&stdio_tools));

    let slash = server.client(&format!("/{WEB}/")).await;
    let answer = (slash.call_tool(call("trace", serde_json::json!({"from": "dashboard"}))))
        .await
        .unwrap();
    assert_eq!(text(&answer), stdio_trace);
    slash.cancel().await.unwrap();

    for path in ["", "/", "/example.com/chain/nope", "/example.com/chain"] {
        let (status, kind, body) = get(&format!("{}{path}", server.url));
        assert_eq!((status, body.as_str()), (404, SERVED), "{path}");
        assert!(kind.starts_with("text/plain"), "{kind}");
        let origin = [("Origin", "http://evil.example")];
        let (status, _, body) = get_with(&format!("{}{path}", server.url), &origin);
        assert_eq!(status, 403, "{path}");
        assert!(!body.contains("example.com/chain"), "{path}: {body}");
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
    assert_golden("chain-web.context-behind.txt", &behind);
    assert!(contents(&indexes) == indexed, "a call wrote an index");
    assert!(run(&dir, "index").2);
    assert_eq!(text(&context().await.unwrap()), fresh);

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
    let url = format!("{}/{WEB}", one.url);
    let proxied = post_with(&url, initialize.clone(), &[("Host", "meada.internal")]);
    assert_eq!(proxied.0, 200, "{proxied:?}");
    let browser = post_with(
        &url,
        initialize.clone(),
        &[("Origin", "http://meada.internal")],
    );
    assert_eq!(browser.0, 403, "{browser:?}");
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
    let kill = Command::new("kill")
        .args(["-TERM", &server.child.id().to_string()])
        .status()
        .unwrap();
    assert!(kill.success());
    assert_eq!(server.child.wait().unwrap().code(), Some(0));
}

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

#[cfg(unix)]
#[tokio::test]
async fn an_answer_in_flight_when_sigterm_arrives_is_received() {
    let dir = chain_manifest("server_drain_in_flight");
    assert!(run(&dir, "index").2);
    let mut server = Server::start(&dir, "127.0.0.1:0");
    let client = server.client(&format!("/{WEB}")).await;
    let commit = commit_in_progress(&dir);
    let answer = tokio::spawn(async move {
        client
            .call_tool(call("trace", serde_json::json!({"from": "dashboard"})))
            .await
    });
    in_flight().await;
    terminate(&server);
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert_eq!(server.child.try_wait().unwrap(), None);
    let address = server.url.trim_start_matches("http://");
    assert!(TcpStream::connect(address).is_err());
    commit.execute_batch("ROLLBACK").unwrap();
    let result = answer.await.unwrap().unwrap();
    assert!(!text(&result).is_empty());
    assert_eq!(
        exit_within(&mut server, Duration::from_secs(5)),
        Some(Some(0))
    );
}

#[cfg(all(unix, debug_assertions))]
#[tokio::test]
async fn the_drain_bound_drops_an_idle_connection_and_an_answer_still_waiting() {
    use std::io::Write;
    let dir = chain_manifest("server_drain_bound");
    assert!(run(&dir, "index").2);
    let mut server = Server::start_with(&dir, "127.0.0.1:0", &[("MEADA_DRAIN_MS", "1000")]);
    let mut idle = TcpStream::connect(server.url.trim_start_matches("http://")).unwrap();
    idle.write_all(b"POST /x HTTP/1.1\r\nHost: x\r\nContent-Le")
        .unwrap();
    let client = server.client(&format!("/{WEB}")).await;
    let _commit = commit_in_progress(&dir);
    let answer = tokio::spawn(async move {
        client
            .call_tool(call("trace", serde_json::json!({"from": "dashboard"})))
            .await
    });
    in_flight().await;
    let start = Instant::now();
    terminate(&server);
    assert_eq!(
        exit_within(&mut server, Duration::from_secs(5)),
        Some(Some(0))
    );
    assert!(
        start.elapsed() >= Duration::from_millis(900),
        "{:?}",
        start.elapsed()
    );
    assert!(answer.await.unwrap().is_err());
}

#[cfg(unix)]
#[tokio::test]
async fn a_second_signal_ends_the_drain_at_once() {
    let dir = chain_manifest("server_drain_second_signal");
    assert!(run(&dir, "index").2);
    let mut server = Server::start(&dir, "127.0.0.1:0");
    let client = server.client(&format!("/{WEB}")).await;
    let _commit = commit_in_progress(&dir);
    let answer = tokio::spawn(async move {
        client
            .call_tool(call("trace", serde_json::json!({"from": "dashboard"})))
            .await
    });
    in_flight().await;
    terminate(&server);
    tokio::time::sleep(Duration::from_millis(700)).await;
    assert_eq!(server.child.try_wait().unwrap(), None);
    terminate(&server);
    assert_eq!(
        exit_within(&mut server, Duration::from_secs(3)),
        Some(Some(0))
    );
    assert!(answer.await.unwrap().is_err());
}
