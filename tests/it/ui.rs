//! The management console's process (spec #293, ADR 0014, feature `ui`): `meada ui --listen
//! <addr> --workspace <bootstrap manifest>` as a child process over a front end's files, talked
//! to in plain HTTP as a browser and a foreign page would.

#![cfg(feature = "ui")]

use std::fs;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::{Value, json};

use crate::manifest::{MANIFEST, run_args};
use crate::server::Server;

/// With `[oidc]`: the console's admission, its routes and the grant bounding every view.
#[cfg(feature = "oidc")]
mod oidc;

/// The playground and the exports over the chain fixture, from `index_dir` or a pulled generation.
pub(super) mod playground;

/// The console's own origin in every bootstrap manifest here.
const PUBLIC: &str = "http://localhost:7999";

/// This build's version, the one the front end's files must name.
const VERSION: &str = env!("CARGO_PKG_VERSION");

const INDEX: &str = "<!doctype html><title>Meada</title>\n";

/// The exact content security policy of every answer without `[oidc]`.
const CSP: &str = "default-src 'self'; script-src 'self' 'wasm-unsafe-eval'; connect-src 'self'; \
                   frame-ancestors 'none'";

/// A file outside `[ui] dir` that no answer may carry.
const OUTSIDE: &str = "outside-the-ui-dir-4f1c";

/// A console's directory: `ui/` with a few files of a front end built for `version` (none
/// without one), `outside.txt` beside it with symbolic links to it and to its directory from
/// inside, and `ops/meada.workspace.toml` with `head`, then `[ui]` with `ui` as its body.
pub(super) fn console_dir(case: &str, version: Option<&str>, head: &str, ui: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(case);
    let _ = fs::remove_dir_all(&dir);
    let files = front_end(&dir, version);
    fs::write(dir.join("outside.txt"), OUTSIDE).unwrap();
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(dir.join("outside.txt"), files.join("leak.txt")).unwrap();
        std::os::unix::fs::symlink(&dir, files.join("up")).unwrap();
    }
    write_manifest(&dir, head, ui);
    dir
}

/// `<dir>/ui/`, a few files of a front end built for `version` (none without one). Returns it.
pub(super) fn front_end(dir: &Path, version: Option<&str>) -> PathBuf {
    let files = dir.join("ui");
    fs::create_dir_all(files.join("assets")).unwrap();
    fs::write(files.join("index.html"), INDEX).unwrap();
    fs::write(files.join("assets/app-1a2b3c.js"), "console.log(1);\n").unwrap();
    fs::write(files.join("assets/app-1a2b3c.wasm"), b"\0asm").unwrap();
    fs::write(files.join("robots.txt"), "User-agent: *\n").unwrap();
    fs::write(files.join("notes.toml"), "x = 1\n").unwrap();
    if let Some(version) = version {
        let built = json!({ "version": version }).to_string();
        fs::write(files.join("meada-ui.json"), built).unwrap();
    }
    files
}

pub(super) fn write_manifest(dir: &Path, head: &str, ui: &str) {
    fs::create_dir_all(dir.join("ops")).unwrap();
    let text = format!("index_dir = \"../indexes\"\n{head}\n[ui]\n{ui}");
    fs::write(dir.join(MANIFEST), text).unwrap();
}

/// The `[ui]` body every console here starts with.
pub(super) fn ui_section() -> String {
    format!("dir = \"../ui\"\npublic_url = \"{PUBLIC}\"\n")
}

/// `meada ui --listen <listen> --workspace ops/meada.workspace.toml` running in `dir`.
pub(super) fn start(dir: &Path, listen: &str) -> Server {
    Server::start_command(
        dir,
        "ui",
        &["--listen", listen, "--workspace", MANIFEST],
        &[],
    )
}

/// An answer read off the wire.
pub(super) struct Reply {
    pub(super) status: u16,
    /// Names in lowercase.
    pub(super) headers: Vec<(String, String)>,
    pub(super) body: String,
}

impl Reply {
    pub(super) fn header(&self, name: &str) -> Option<&str> {
        let found = self.headers.iter().find(|(n, _)| n == name);
        found.map(|(_, v)| v.as_str())
    }

    pub(super) fn json(&self) -> Value {
        serde_json::from_str(&self.body).unwrap_or_else(|_| panic!("JSON: {}", self.body))
    }

    /// No cookie and no CORS header: no page of another origin reads an answer.
    pub(super) fn assert_no_cookie_or_cors(&self) {
        for (name, _) in &self.headers {
            assert!(
                name != "set-cookie" && !name.starts_with("access-control-"),
                "{name}"
            );
        }
    }
}

/// `method path` over a connection of its own to `console`, with `headers` (a `Host` of the
/// bound address unless they name one), sent as written: nothing normalises the path.
pub(super) fn send(console: &Server, method: &str, path: &str, headers: &[(&str, &str)]) -> Reply {
    send_body(console, method, path, headers, "")
}

/// `send` with `body`.
pub(super) fn send_body(
    console: &Server,
    method: &str,
    path: &str,
    headers: &[(&str, &str)],
    body: &str,
) -> Reply {
    let address = console.url.trim_start_matches("http://");
    let mut stream = TcpStream::connect(address).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(30)))
        .unwrap();
    let mut request = format!("{method} {path} HTTP/1.1\r\nConnection: close\r\n");
    if !headers.iter().any(|(n, _)| n.eq_ignore_ascii_case("host")) {
        request += &format!("Host: {address}\r\n");
    }
    for (name, value) in headers {
        request += &format!("{name}: {value}\r\n");
    }
    request += &format!("Content-Length: {}\r\n\r\n{body}", body.len());
    stream.write_all(request.as_bytes()).unwrap();
    let mut raw = Vec::new();
    stream.read_to_end(&mut raw).unwrap();
    let raw = String::from_utf8_lossy(&raw).into_owned();
    let (head, body) = raw.split_once("\r\n\r\n").expect("an HTTP answer");
    let mut lines = head.lines();
    let status = lines.next().unwrap().split(' ').nth(1).unwrap();
    let headers = lines
        .filter_map(|l| l.split_once(':'))
        .map(|(n, v)| (n.trim().to_ascii_lowercase(), v.trim().to_string()))
        .collect();
    Reply {
        status: status.parse().unwrap(),
        headers,
        body: body.to_string(),
    }
}

pub(super) fn get(console: &Server, path: &str) -> Reply {
    send(console, "GET", path, &[])
}

/// The three read routes answer their documents; an unknown API path is the JSON 404; the
/// `manifest` route reports the file as it now stands on disk.
#[test]
fn serves_config_session_and_manifest() {
    let dir = console_dir("ui_routes", Some(VERSION), "", &ui_section());
    let console = start(&dir, "127.0.0.1:0");

    let config = get(&console, "/api/v1/config");
    assert_eq!(config.status, 200);
    assert_eq!(config.header("content-type"), Some("application/json"));
    assert_eq!(
        config.json(),
        json!({"version": VERSION, "operations": false})
    );

    let session = get(&console, "/api/v1/session");
    assert_eq!(session.json(), json!({"subject": null, "role": "viewer"}));

    let manifest = get(&console, "/api/v1/manifest").json();
    assert_eq!(manifest["file"], json!(MANIFEST));
    assert_eq!(
        manifest["manifest"],
        json!({"index_dir": "../indexes", "ui": {"dir": "../ui", "public_url": PUBLIC}})
    );
    assert_eq!(manifest["errors"], json!([]));
    let modules = manifest["modules"].as_array().unwrap();
    let sections: Vec<&str> = modules
        .iter()
        .map(|m| m["section"].as_str().unwrap())
        .collect();
    assert_eq!(
        sections,
        [
            "valkey",
            "snapshot",
            "generations",
            "refresh_lock",
            "oidc",
            "github_app",
            "ui"
        ]
    );
    for module in modules {
        let built = match module["feature"].as_str().unwrap() {
            "valkey" => cfg!(feature = "valkey"),
            "snapshot" => cfg!(feature = "snapshot"),
            "generation-pointer" => cfg!(feature = "generation-pointer"),
            "refresh-lock" => cfg!(feature = "refresh-lock"),
            "oidc" => cfg!(feature = "oidc"),
            "github-app" => cfg!(feature = "github-app"),
            "ui" => true,
            other => panic!("{other}"),
        };
        assert_eq!(module["built"], json!(built), "{module}");
    }

    let unknown = send(&console, "GET", "/api/v1/nothing", &[("Origin", PUBLIC)]);
    assert_eq!(unknown.status, 404);
    assert_eq!(
        unknown.json(),
        json!({"error": {"code": "not_found", "message": "no API route at this path"}})
    );

    // Broken on disk after the start: the next answer names the file, the line and the key,
    // beside the configuration the console started with.
    write_manifest(&dir, "", &(ui_section() + "colour = \"blue\"\n"));
    let manifest = get(&console, "/api/v1/manifest").json();
    assert_eq!(
        manifest["errors"],
        json!([format!(
            "{MANIFEST}:6: `ui.colour` is not a meada.workspace.toml key"
        )])
    );
    assert_eq!(manifest["manifest"]["ui"]["dir"], json!("../ui"));

    for reply in [config, session, unknown] {
        reply.assert_no_cookie_or_cors();
    }
}

/// The address forms of `meada mcp --listen`, a port alone on loopback; both flags required.
#[test]
fn takes_the_listen_forms_of_the_node_and_needs_both_flags() {
    let dir = console_dir("ui_listen", Some(VERSION), "", &ui_section());
    for listen in ["0", ":0", "localhost:0", "127.0.0.1:0"] {
        let console = start(&dir, listen);
        assert!(console.url.starts_with("http://127.0.0.1:"), "{listen}");
        assert_eq!(get(&console, "/api/v1/session").status, 200, "{listen}");
    }
    let (_, stderr, code) = run_args(
        &dir,
        &[
            "ui",
            "--listen",
            "host.example:7999",
            "--workspace",
            MANIFEST,
        ],
    );
    assert_eq!(code, Some(1));
    assert!(stderr.contains("`--listen host.example:7999`"), "{stderr}");
    for args in [
        &["ui", "--listen", "0"][..],
        &["ui", "--workspace", MANIFEST],
    ] {
        let (_, stderr, code) = run_args(&dir, args);
        assert_eq!(code, Some(2), "{args:?}: {stderr}");
    }
}

/// The `Host` check, then the `Origin` check, each with its fixed 403 body; a write needs an
/// `Origin`.
#[test]
fn refuses_a_foreign_host_a_foreign_origin_and_a_write_without_origin() {
    let dir = console_dir("ui_checks", Some(VERSION), "", &ui_section());
    let console = start(&dir, "127.0.0.1:0");
    let host = "Forbidden: Host header is not allowed\n";
    let origin = "Forbidden: Origin header is not allowed\n";
    let write = "Forbidden: a write needs an Origin header\n";
    let foreign_host = ("Host", "evil.example");
    let foreign_origin = ("Origin", "http://evil.example");
    for (method, headers, body) in [
        ("GET", &[foreign_host][..], host),
        ("GET", &[foreign_origin], origin),
        ("POST", &[], write),
        ("POST", &[foreign_host, foreign_origin], host),
        ("GET", &[foreign_host, foreign_origin], host),
        // `public_url`'s origin with another port is another origin.
        ("GET", &[("Origin", "http://localhost:8000")], origin),
    ] {
        let reply = send(&console, method, "/api/v1/config", headers);
        assert_eq!(
            (reply.status, reply.body.as_str()),
            (403, body),
            "{headers:?}"
        );
        assert_eq!(reply.header("content-security-policy"), Some(CSP));
        reply.assert_no_cookie_or_cors();
    }
    // `public_url`'s host and port, and the console's own origin, pass.
    for headers in [
        &[("Host", "localhost:7999")][..],
        &[("Host", "localhost:7999"), ("Origin", PUBLIC)],
        &[("Origin", PUBLIC)],
    ] {
        let reply = send(&console, "GET", "/api/v1/config", headers);
        assert_eq!(reply.status, 200, "{headers:?}");
    }
    // A write with the console's own origin reaches routing: no write route yet.
    let reply = send(&console, "POST", "/api/v1/config", &[("Origin", PUBLIC)]);
    assert_eq!(reply.status, 404);
    assert_eq!(reply.json()["error"]["code"], json!("not_found"));
}

/// Static files: their headers, the cache rules, no traversal, no listing, the front end's
/// routes answered with `index.html`.
#[test]
fn serves_the_front_end_s_files_and_nothing_else() {
    let dir = console_dir("ui_static", Some(VERSION), "", &ui_section());
    let console = start(&dir, "127.0.0.1:0");
    for (path, kind, cache, body) in [
        ("/", "text/html; charset=utf-8", "no-cache", INDEX),
        ("/index.html", "text/html; charset=utf-8", "no-cache", INDEX),
        (
            "/assets/app-1a2b3c.js",
            "text/javascript; charset=utf-8",
            "public, max-age=31536000, immutable",
            "console.log(1);\n",
        ),
        (
            "/assets/app-1a2b3c.wasm",
            "application/wasm",
            "public, max-age=31536000, immutable",
            "\0asm",
        ),
        (
            "/robots.txt",
            "text/plain; charset=utf-8",
            "no-cache",
            "User-agent: *\n",
        ),
        // The front end's own routes, and a directory: `index.html`, never a listing.
        (
            "/repositories/web",
            "text/html; charset=utf-8",
            "no-cache",
            INDEX,
        ),
        ("/assets", "text/html; charset=utf-8", "no-cache", INDEX),
    ] {
        let reply = get(&console, path);
        assert_eq!(reply.status, 200, "{path}");
        assert_eq!(reply.header("content-type"), Some(kind), "{path}");
        assert_eq!(reply.header("cache-control"), Some(cache), "{path}");
        assert_eq!(reply.header("x-content-type-options"), Some("nosniff"));
        assert_eq!(reply.header("referrer-policy"), Some("no-referrer"));
        assert_eq!(reply.header("content-security-policy"), Some(CSP));
        assert_eq!(reply.body, body, "{path}");
        reply.assert_no_cookie_or_cors();
    }
    let mut refused = vec![
        "/..",
        "/../outside.txt",
        "/%2e%2e/outside.txt",
        "/assets/%2E%2E/%2e%2e/outside.txt",
        "/assets//app-1a2b3c.js",
        "/assets/",
        "/notes.toml",
        "/api/nothing",
    ];
    if cfg!(unix) {
        refused.extend(["/leak.txt", "/up/outside.txt"]);
    }
    for path in refused {
        let reply = get(&console, path);
        assert_eq!(reply.status, 404, "{path}");
        assert!(!reply.body.contains(OUTSIDE), "{path}");
        assert_eq!(reply.header("content-security-policy"), Some(CSP));
        reply.assert_no_cookie_or_cors();
    }
}

/// Every problem that refuses the start, listed at once.
#[test]
fn refuses_to_start_naming_every_problem() {
    let at = format!("meada: {MANIFEST}");
    let cases = vec![
        (
            "ui_refuse_keys",
            Some(VERSION),
            String::new(),
            "operations = false\n".to_string(),
            vec![
                format!("{at}:3: `ui`: `ui.dir` is missing"),
                format!("{at}:3: `ui`: `ui.public_url` is missing"),
            ],
        ),
        (
            "ui_refuse_http",
            Some(VERSION),
            String::new(),
            "dir = \"../ui\"\npublic_url = \"http://console.example\"\n".to_string(),
            vec![format!(
                "{at}:5: `ui.public_url`: must be an https:// URL with a host and nothing after \
                 it (http:// only on loopback)"
            )],
        ),
        (
            "ui_refuse_path",
            Some(VERSION),
            String::new(),
            "dir = \"../ui\"\npublic_url = \"https://console.example/ui\"\n".to_string(),
            vec![format!("{at}:5: `ui.public_url`: must be an https:// URL")],
        ),
        (
            "ui_refuse_unknown",
            Some(VERSION),
            String::new(),
            ui_section() + "colour = \"blue\"\n",
            vec![format!(
                "{at}:6: `ui.colour` is not a meada.workspace.toml key"
            )],
        ),
        (
            "ui_refuse_dir",
            Some(VERSION),
            String::new(),
            format!("dir = \"../missing\"\npublic_url = \"{PUBLIC}\"\n"),
            vec![format!(
                "{at}:4: `ui.dir`: `ops/../missing` is not a directory"
            )],
        ),
        (
            "ui_refuse_version",
            Some("0.0.0-other"),
            String::new(),
            ui_section(),
            vec![format!(
                "{at}:4: `ui.dir`: `ops/../ui` holds files built for Meada 0.0.0-other, not \
                 {VERSION}"
            )],
        ),
        (
            "ui_refuse_unversioned",
            None,
            String::new(),
            ui_section(),
            vec![format!(
                "{at}:4: `ui.dir`: `ops/../ui` has no readable meada-ui.json naming its \
                 version; this is Meada {VERSION}"
            )],
        ),
    ];
    for (case, version, head, ui, expected) in cases {
        let dir = console_dir(case, version, &head, &ui);
        let (stdout, stderr, code) = run_args(
            &dir,
            &["ui", "--listen", "127.0.0.1:0", "--workspace", MANIFEST],
        );
        assert_eq!(code, Some(1), "{case}: {stderr}");
        assert!(stdout.is_empty() && !stderr.contains("listening"), "{case}");
        let lines: Vec<&str> = stderr.lines().collect();
        assert_eq!(lines.len(), expected.len(), "{case}: {stderr}");
        for (line, expected) in lines.iter().zip(&expected) {
            assert!(line.starts_with(expected.as_str()), "{case}: {line}");
        }
    }
    // A bootstrap manifest with no `[ui]`.
    let dir = console_dir("ui_refuse_section", Some(VERSION), "", &ui_section());
    fs::write(dir.join(MANIFEST), "index_dir = \"../indexes\"\n").unwrap();
    let (_, stderr, code) = run_args(
        &dir,
        &["ui", "--listen", "127.0.0.1:0", "--workspace", MANIFEST],
    );
    assert_eq!(code, Some(1));
    assert_eq!(
        stderr,
        format!("meada: {MANIFEST}: the bootstrap manifest has no [ui] section\n")
    );
}

/// The `manifest` route names a secret's file, never what it holds; no answer carries it.
#[cfg(feature = "valkey")]
#[test]
fn no_answer_carries_a_secret() {
    let secret = "s3cret-valkey-password-91d2";
    let head =
        "\n[valkey]\nurl = \"redis://127.0.0.1:6390\"\npassword_file = \"../valkey-password\"\n";
    let dir = console_dir("ui_secret", Some(VERSION), head, &ui_section());
    fs::write(dir.join("valkey-password"), secret).unwrap();
    let console = start(&dir, "127.0.0.1:0");
    let manifest = get(&console, "/api/v1/manifest").json();
    assert_eq!(
        manifest["manifest"]["valkey"]["password_file"],
        json!("../valkey-password")
    );
    for path in [
        "/api/v1/config",
        "/api/v1/session",
        "/api/v1/manifest",
        "/api/v1/nothing",
        "/",
        "/valkey-password",
        "/../valkey-password",
        "/%2e%2e/valkey-password",
    ] {
        let reply = get(&console, path);
        assert!(!reply.body.contains(secret), "{path}");
        assert!(
            reply.headers.iter().all(|(_, v)| !v.contains(secret)),
            "{path}"
        );
    }
}

/// SIGTERM during a slow request (the `manifest` route reading a FIFO no one writes yet): the
/// listener closes, the answer is sent once the read ends, and the process exits 0.
#[cfg(unix)]
#[test]
fn an_answer_in_flight_when_sigterm_arrives_is_received() {
    use crate::server::{exit_within, terminate};
    let dir = console_dir("ui_drain", Some(VERSION), "", &ui_section());
    let mut console = start(&dir, "127.0.0.1:0");
    let manifest = dir.join(MANIFEST);
    let text = fs::read_to_string(&manifest).unwrap();
    fs::remove_file(&manifest).unwrap();
    let fifo = std::process::Command::new("mkfifo")
        .arg(&manifest)
        .status()
        .unwrap();
    assert!(fifo.success());
    let address = console.url.clone();
    let answer = std::thread::scope(|scope| {
        let answer = scope.spawn(|| get(&console, "/api/v1/manifest"));
        // A margin for the request to reach the console and block on the FIFO.
        std::thread::sleep(Duration::from_millis(1500));
        terminate(&console);
        std::thread::sleep(Duration::from_millis(500));
        assert!(TcpStream::connect(address.trim_start_matches("http://")).is_err());
        // The FIFO's writer: the read ends and the answer goes out.
        fs::write(&manifest, &text).unwrap();
        answer.join().unwrap()
    });
    assert_eq!(answer.status, 200);
    assert_eq!(answer.json()["errors"], json!([]));
    assert_eq!(
        exit_within(&mut console, Duration::from_secs(5)),
        Some(Some(0))
    );
}
