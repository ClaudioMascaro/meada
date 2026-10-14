//! `meada decide --decider` (spec #18, ADR 0006) against the loopback stub: the configuration
//! from the environment, the `chat` requests, `--show-request`, and the network boundary (no
//! other command reaches the decider). Every run asserts that the key appears in no output.

mod common;

use std::path::Path;
use std::process::{Command, Output, Stdio};

use common::stub::{Reply, Stub};
use common::{assert_golden, chain, fixture_repo};

/// The key every test configures. No output of any test may contain it.
const KEY: &str = "sk-meada-test-5f0c1e2d9a";

const VARS: [&str; 4] = [
    "MEADA_DECIDER_PROTOCOL",
    "MEADA_DECIDER_URL",
    "MEADA_DECIDER_KEY",
    "MEADA_DECIDER_MODEL",
];

const PROXIES: [&str; 3] = ["HTTP_PROXY", "HTTPS_PROXY", "ALL_PROXY"];

/// Runs `meada <args>` in `repo` with only the decider variables in `env` set (the inherited
/// ones are removed) and `stdin` as input. Returns stdout, stderr and success, with the
/// repository's path replaced by `<repo>` and `stub` (when given) by `<stub>`, after checking
/// that the key is in neither stream.
fn meada(
    repo: &Path,
    args: &[&str],
    env: &[(&str, String)],
    stdin: &str,
    stub: Option<&Stub>,
) -> (String, String, bool) {
    let mut command = Command::new(env!("CARGO_BIN_EXE_meada"));
    command
        .current_dir(repo)
        .args(args)
        .env("LANG", "en_US.UTF-8");
    // No inherited proxy either: the test key must not travel anywhere but to the stub.
    for var in VARS.iter().chain(&PROXIES) {
        command.env_remove(var);
        command.env_remove(var.to_ascii_lowercase());
    }
    command.envs(env.iter().map(|(k, v)| (k, v)));
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("meada runs");
    use std::io::Write;
    child
        .stdin
        .take()
        .unwrap()
        .write_all(stdin.as_bytes())
        .unwrap();
    let Output {
        status,
        stdout,
        stderr,
    } = child.wait_with_output().unwrap();
    let text = |bytes: &[u8]| {
        let text = String::from_utf8_lossy(bytes);
        assert!(!text.contains(KEY), "the key leaked:\n{text}");
        let text = text.replace(repo.to_str().unwrap(), "<repo>");
        match stub {
            Some(stub) => text.replace(&stub.url, "<stub>"),
            None => text,
        }
    };
    (text(&stdout), text(&stderr), status.success())
}

/// The `chat` configuration against `stub`, with the key.
fn chat(stub: &Stub) -> Vec<(&'static str, String)> {
    vec![
        ("MEADA_DECIDER_PROTOCOL", "chat".into()),
        ("MEADA_DECIDER_URL", format!("{}/v1", stub.url)),
        ("MEADA_DECIDER_KEY", KEY.into()),
        ("MEADA_DECIDER_MODEL", "local/test-model".into()),
    ]
}

#[test]
fn show_request_prints_the_golden_chat_requests_and_sends_nothing() {
    let (web, _, _) = chain("decider_show");
    let stub = Stub::start(vec![]);

    let env = chat(&stub);
    let (stdout, stderr, ok) = meada(
        &web,
        &["decide", "--decider", "--show-request"],
        &env,
        "",
        Some(&stub),
    );

    assert!(ok, "{stderr}");
    assert_golden("chain-web.decide.show-request.chat.txt", &stdout);
    assert!(stub.requests().is_empty());
    assert!(!web.join("meada.decisions.toml").exists());
}

#[test]
fn the_url_must_use_https_except_to_a_loopback_host() {
    let (web, _, _) = chain("decider_urls");
    let stub = Stub::start(vec![]);
    let port = stub.url.rsplit(':').next().unwrap().to_string();
    let show = ["decide", "--decider", "--show-request"];
    let rejected = [
        "http://localhost@evil.com".to_string(),
        "http://127.0.0.1.evil.com".into(),
        "http://0.0.0.0".into(),
        "http://localhost.".into(),
        "http://[::ffff:127.0.0.1]".into(),
        "HTTP://example.com".into(),
    ];
    let accepted = [
        format!("http://[::1]:{port}"),
        format!("http://user@127.0.0.1:{port}"),
        format!("HTTP://127.0.0.1:{port}"),
        "HTTPS://example.com/v1".into(),
    ];

    for (urls, ok) in [(&rejected[..], false), (&accepted[..], true)] {
        for url in urls {
            let mut env = chat(&stub);
            env.retain(|(var, _)| *var != "MEADA_DECIDER_URL");
            env.push(("MEADA_DECIDER_URL", url.clone()));
            let (_, stderr, success) = meada(&web, &show, &env, "", Some(&stub));
            assert_eq!(success, ok, "{url}: {stderr}");
            if !ok {
                assert!(
                    stderr.starts_with("meada: `MEADA_DECIDER_URL` must use https"),
                    "{url}: {stderr}"
                );
            }
        }
    }
    assert!(stub.requests().is_empty());
}

#[test]
fn a_loopback_decider_is_reached_directly_whatever_the_proxy_variables_say() {
    let (web, _, _) = chain("decider_proxy");
    let answer = serde_json::json!({"choice": "1", "probability": 0.5});
    let stub = Stub::start(vec![
        Reply::chat("local/test-model", answer.clone()),
        Reply::chat("local/test-model", answer),
    ]);
    let proxy = Stub::start(vec![]);
    let mut env = chat(&stub);
    for var in PROXIES {
        env.push((var, proxy.url.clone()));
    }

    let (_, stderr, ok) = meada(&web, &["decide", "--decider"], &env, "", Some(&stub));

    assert!(ok, "{stderr}");
    assert_eq!(stub.requests().len(), 2);
    assert!(proxy.requests().is_empty());
}

#[test]
fn chat_sends_one_request_per_item_in_item_order_with_the_key_as_a_bearer_token() {
    let (web, _, _) = chain("decider_send");
    let answer = serde_json::json!({"choice": "2", "probability": 0.9});
    let stub = Stub::start(vec![
        Reply::chat("local/test-model", answer.clone()),
        Reply::chat("local/test-model", answer),
    ]);
    let env = chat(&stub);
    let (shown, _, _) = meada(
        &web,
        &["decide", "--decider", "--show-request"],
        &env,
        "",
        Some(&stub),
    );

    let (_, stderr, ok) = meada(&web, &["decide", "--decider"], &env, "", Some(&stub));

    assert!(ok, "{stderr}");
    let requests = stub.requests();
    assert_eq!(requests.len(), 2);
    for (request, function) in requests.iter().zip(["health", "track"]) {
        assert_eq!(
            (request.method.as_str(), request.path.as_str()),
            ("POST", "/v1/chat/completions")
        );
        let bearer = format!("Bearer {KEY}");
        assert_eq!(request.header("authorization"), Some(bearer.as_str()));
        // What left the machine is what `--show-request` printed.
        let body = serde_json::to_string_pretty(&request.json()).unwrap();
        assert!(shown.contains(&body), "{body}");
        assert!(body.contains(&format!("function {function}(")), "{body}");
    }
}

#[test]
fn the_key_is_optional_for_chat() {
    let (web, _, _) = chain("decider_keyless");
    let stub = Stub::start(vec![]);
    let env: Vec<_> = (chat(&stub).into_iter())
        .filter(|(var, _)| *var != "MEADA_DECIDER_KEY")
        .collect();

    let (shown, stderr, ok) = meada(
        &web,
        &["decide", "--decider", "--show-request"],
        &env,
        "",
        Some(&stub),
    );
    assert!(ok, "{stderr}");
    assert!(!shown.contains("Authorization"), "{shown}");

    meada(&web, &["decide", "--decider"], &env, "", Some(&stub));
    let requests = stub.requests();
    assert_eq!(requests.len(), 2);
    assert!(requests.iter().all(|r| r.header("authorization").is_none()));
}

#[test]
fn a_failed_request_is_reported_with_its_item() {
    let (web, _, _) = chain("decider_fail");
    // The first answer echoes the key back: it must not reach any output either.
    let stub = Stub::start(vec![
        Reply::status(500, KEY),
        Reply::status(401, "unauthorized"),
    ]);

    let (stdout, stderr, ok) = meada(
        &web,
        &["decide", "--decider"],
        &chat(&stub),
        "",
        Some(&stub),
    );

    assert!(ok, "{stderr}");
    assert!(
        stdout.contains(
            "example.com/chain/web::src/status.ts::health::fetch#0\n\
             request failed: http status: 500\n"
        ),
        "{stdout}"
    );
    assert!(
        stdout.contains("request failed: http status: 401\n"),
        "{stdout}"
    );
    assert!(!web.join("meada.decisions.toml").exists());
}

#[test]
fn a_missing_or_invalid_variable_is_an_error_naming_it_before_any_request() {
    let (web, _, _) = chain("decider_config");
    let stub = Stub::start(vec![]);
    let url = format!("{}/v1", stub.url);
    let not_yet = "the `decisions` protocol of `MEADA_DECIDER_PROTOCOL` is not available yet";
    fn chat_to(url: &str) -> Vec<(&'static str, &str)> {
        vec![
            ("MEADA_DECIDER_PROTOCOL", "chat"),
            ("MEADA_DECIDER_URL", url),
            ("MEADA_DECIDER_MODEL", "m"),
        ]
    }
    let mut spaced = chat_to("http://localhost:1");
    spaced.push(("MEADA_DECIDER_KEY", "two words"));
    let cases: Vec<(Vec<(&str, &str)>, &str)> = vec![
        (
            vec![("MEADA_DECIDER_PROTOCOL", "grpc")],
            "`MEADA_DECIDER_PROTOCOL` must be `decisions` or `chat`",
        ),
        (
            vec![("MEADA_DECIDER_PROTOCOL", "chat")],
            "`MEADA_DECIDER_URL` is missing",
        ),
        (
            vec![
                ("MEADA_DECIDER_PROTOCOL", "chat"),
                ("MEADA_DECIDER_URL", &url),
            ],
            "`MEADA_DECIDER_MODEL` is missing",
        ),
        (
            chat_to("http://example.com/v1"),
            "`MEADA_DECIDER_URL` must use https",
        ),
        (
            chat_to("ftp://127.0.0.1/v1"),
            "`MEADA_DECIDER_URL` must use https",
        ),
        (spaced, "`MEADA_DECIDER_KEY` is not valid"),
        // `decisions` is the default protocol: the URL and the model have defaults, the key
        // has none.
        (vec![], "`MEADA_DECIDER_KEY` is missing"),
        (vec![("MEADA_DECIDER_KEY", KEY)], not_yet),
        (
            vec![
                ("MEADA_DECIDER_PROTOCOL", "decisions"),
                ("MEADA_DECIDER_URL", &url),
                ("MEADA_DECIDER_KEY", KEY),
            ],
            not_yet,
        ),
    ];

    for (env, error) in cases {
        let env: Vec<_> = env.iter().map(|(k, v)| (*k, v.to_string())).collect();
        for args in [
            &["decide", "--decider"][..],
            &["decide", "--decider", "--show-request"],
        ] {
            let (stdout, stderr, ok) = meada(&web, args, &env, "", Some(&stub));
            assert!(!ok, "{stdout}");
            assert!(stderr.starts_with(&format!("meada: {error}")), "{stderr}");
        }
    }
    assert!(stub.requests().is_empty());
}

#[test]
fn an_item_with_more_than_fifty_candidates_is_skipped_in_decider_mode_only() {
    let origin = Some("https://example.com/open/generated.git");
    let repo = fixture_repo("decider_many", "open-generated", origin);
    let stub = Stub::start(vec![]);

    let (stdout, stderr, ok) = meada(
        &repo,
        &["decide", "--decider", "--show-request"],
        &chat(&stub),
        "",
        Some(&stub),
    );

    assert!(ok, "{stderr}");
    assert_eq!(
        stdout,
        "example.com/open/generated::src/client.ts::item::fetch#0\n\
         55 candidates, more than 50; skipped in decider mode\n"
    );
    // Human mode still offers it.
    let (stdout, _, ok) = meada(&repo, &["decide"], &[], "s\n", None);
    assert!(ok);
    assert!(stdout.contains("  55) "), "{stdout}");
    assert!(stub.requests().is_empty());
}

#[test]
fn list_prune_and_human_mode_read_no_decider_variable_and_send_nothing() {
    let (web, _, _) = chain("decider_list");
    let stub = Stub::start(vec![]);
    let mut env = chat(&stub);
    let list = ["decide", "--decider", "--list"];
    let prune = ["decide", "--decider", "--prune"];

    for args in [list, prune] {
        let (_, stderr, ok) = meada(&web, &args, &env, "", Some(&stub));
        assert!(ok, "{stderr}");
    }

    // Invalid variables do not matter where nothing reads them.
    env.push(("MEADA_DECIDER_PROTOCOL", "grpc".into()));
    for args in [list, prune] {
        let (_, stderr, ok) = meada(&web, &args, &env, "", Some(&stub));
        assert!(ok, "{stderr}");
    }
    let (_, stderr, ok) = meada(&web, &["decide", "--dry-run"], &env, "1\n1\n", Some(&stub));
    assert!(ok, "{stderr}");
    assert!(stub.requests().is_empty());
}

#[test]
fn no_other_command_reaches_the_decider() {
    let (web, _, _) = chain("decider_boundary");
    let stub = Stub::start(vec![]);
    let env = chat(&stub);

    for command in ["index", "export", "status"] {
        let (_, stderr, ok) = meada(&web, &[command], &env, "", Some(&stub));
        assert!(ok, "{command}: {stderr}");
    }

    assert!(stub.requests().is_empty());
}

/// The MCP server, with every decider variable set, answers each of its tools and sends
/// nothing to the decider.
#[tokio::test]
async fn the_mcp_server_never_reaches_the_decider() {
    use rmcp::{
        ServiceExt,
        model::CallToolRequestParams,
        transport::{ConfigureCommandExt, TokioChildProcess},
    };
    let (web, _, _) = chain("decider_mcp");
    let stub = Stub::start(vec![]);
    let env = chat(&stub);
    let transport = TokioChildProcess::new(
        tokio::process::Command::new(env!("CARGO_BIN_EXE_meada")).configure(|c| {
            c.arg("mcp")
                .current_dir(&web)
                .envs(env.iter().map(|(k, v)| (k, v)));
        }),
    )
    .unwrap();
    let client = ().serve(transport).await.unwrap();

    let calls = [
        (
            "search",
            serde_json::json!({"query": "", "kind": "ambiguous"}),
        ),
        ("context", serde_json::json!({"symbol": "health"})),
        (
            "trace",
            serde_json::json!({"from": "health", "direction": "down"}),
        ),
    ];
    for (tool, arguments) in calls {
        let answer = client
            .call_tool(
                CallToolRequestParams::new(tool)
                    .with_arguments(arguments.as_object().unwrap().clone()),
            )
            .await
            .unwrap();
        let text = answer.content.first().and_then(|c| c.as_text()).unwrap();
        assert!(!text.text.contains(KEY), "{}", text.text);
    }
    client.cancel().await.unwrap();

    assert!(stub.requests().is_empty());
}
