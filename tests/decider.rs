//! `meada decide --decider` (spec #18, ADR 0006) against the loopback stub: the configuration
//! from the environment, the `chat` requests, `--show-request`, and the network boundary (no
//! other command reaches the decider). Every run asserts that the key appears in no output.

mod common;

use std::fs;
use std::path::Path;
use std::process::{Command, Output, Stdio};

use meada::engine::Engine;

use common::stub::{Reply, Stub};
use common::{assert_golden, chain, fixture_repo};

/// The key every test configures. No output of any test may contain it. The `&` lets a JSON
/// answer write it escaped (`&`), out of reach of a redaction of the raw body.
const KEY: &str = "sk-meada&test-5f0c1e2d9a";

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

/// The `decisions` configuration against `stub`: the protocol and the model are the defaults.
fn decisions(stub: &Stub) -> Vec<(&'static str, String)> {
    vec![
        ("MEADA_DECIDER_URL", stub.url.clone()),
        ("MEADA_DECIDER_KEY", KEY.into()),
    ]
}

#[test]
fn show_request_prints_the_golden_decisions_request_and_sends_nothing() {
    let (web, _, _) = chain("decider_show_decisions");
    let stub = Stub::start(vec![]);
    let show = ["decide", "--decider", "--show-request"];

    let (stdout, stderr, ok) = meada(&web, &show, &decisions(&stub), "", Some(&stub));

    assert!(ok, "{stderr}");
    assert_golden("chain-web.decide.show-request.decisions.txt", &stdout);
    // With only the key set, the request would go to OpenRouter; `--show-request` sends nothing.
    let key = [("MEADA_DECIDER_KEY", KEY.to_string())];
    let (stdout, stderr, ok) = meada(&web, &show, &key, "", None);
    assert!(ok, "{stderr}");
    assert!(
        stdout.starts_with("POST https://openrouter.ai/api/alpha/decisions\n"),
        "{stdout}"
    );
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

    assert!(!ok);
    assert!(stderr.contains("2 item(s) failed"), "{stderr}");
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

const HEALTH: &str = "example.com/chain/web::src/status.ts::health::fetch#0";
const TRACK: &str = "example.com/chain/web::src/status.ts::track::fetch#0";

fn answer(choice: &str, probability: f64) -> serde_json::Value {
    serde_json::json!({"choice": choice, "probability": probability})
}

fn written(repo: &Path) -> Option<String> {
    fs::read_to_string(repo.join("meada.decisions.toml")).ok()
}

#[test]
fn chat_answers_are_written_with_the_response_model_and_p_and_replayed_as_decided_links() {
    let (web, _, _) = chain("decider_write");
    // The health check is the second candidate (gateway); the tracking URL the first (catalog).
    // The response names a dated model id, which is the one recorded.
    let stub = Stub::start(vec![
        Reply::chat("local/test-model-2026", answer("2", 0.9)),
        Reply::chat("local/test-model-2026", answer("1", 0.62)),
    ]);

    let (stdout, stderr, ok) = meada(
        &web,
        &["decide", "--decider"],
        &chat(&stub),
        "",
        Some(&stub),
    );

    assert!(ok, "{stderr}");
    assert_golden("chain-web.decider.chat.en.txt", &stdout);
    let file = written(&web).expect("the file is written");
    assert_golden("chain-web.decisions.chat.toml", &file);
    // A cold index of the chain with the file gives the decided export.
    let (cold, _, _) = chain("decider_write_cold");
    fs::write(cold.join("meada.decisions.toml"), &file).unwrap();
    let export = Engine::open(&cold).unwrap().export().unwrap();
    assert_golden("chain-web.decided-chat.export.json", &export);
}

/// The `chat` answer whose message content is `content`, as the model's text.
fn raw(content: &str) -> Reply {
    Reply::ok(serde_json::json!({
        "model": "local/test-model",
        "choices": [{"message": {"role": "assistant", "content": content}}],
    }))
}

#[test]
fn a_malformed_answer_fails_its_item_alone_and_the_others_are_still_written() {
    let valid = || Reply::chat("local/test-model", answer("1", 0.8));
    let cases = [
        ("not json", Reply::status(200, "<html>"), "unreadable"),
        (
            "no choices",
            Reply::ok(serde_json::json!({"choices": []})),
            "no choices",
        ),
        (
            "no content",
            Reply::ok(serde_json::json!({"choices": [{"message": {}}]})),
            "unreadable",
        ),
        ("prose", raw("I think the second one."), "unreadable"),
        (
            "no choice",
            raw(r#"{"probability": 0.9}"#),
            "missing field `choice`",
        ),
        (
            "no probability",
            raw(r#"{"choice": "1"}"#),
            "missing field `probability`",
        ),
        (
            "text probability",
            raw(r#"{"choice": "1", "probability": "high"}"#),
            "unreadable",
        ),
        (
            "unoffered key",
            Reply::chat("local/test-model", answer("3", 0.9)),
            "choice `3` is not one of the offered keys (1, 2)",
        ),
        (
            "probability above one",
            Reply::chat("local/test-model", answer("1", 1.5)),
            "probability 1.5 is not between 0 and 1",
        ),
        (
            "negative probability",
            Reply::chat("local/test-model", answer("1", -0.1)),
            "probability -0.1 is not between 0 and 1",
        ),
        (
            "model id human",
            Reply::chat("human", answer("1", 0.9)),
            "model id \"human\" cannot be recorded",
        ),
        (
            "model id with control characters",
            Reply::chat("evil\u{1b}[2J", answer("1", 0.9)),
            "model id \"evil[2J\" cannot be recorded",
        ),
    ];

    for (name, bad, reason) in cases {
        let (web, _, _) = chain(&format!("decider_bad_{}", name.replace(' ', "_")));
        let stub = Stub::start(vec![bad, valid()]);

        let (stdout, stderr, ok) = meada(
            &web,
            &["decide", "--decider"],
            &chat(&stub),
            "",
            Some(&stub),
        );

        assert!(!ok, "{name}");
        assert!(stderr.contains("1 item(s) failed"), "{name}: {stderr}");
        let report = format!("{HEALTH}\ninvalid answer: ");
        let at = stdout
            .find(&report)
            .unwrap_or_else(|| panic!("{name}: {stdout}"));
        assert!(
            stdout[at + report.len()..].contains(reason),
            "{name}: {stdout}"
        );
        // Nothing for the failed item; the other item's decision is written.
        let file = written(&web).unwrap_or_else(|| panic!("{name}: no file\n{stdout}"));
        assert!(!file.contains(HEALTH), "{name}: {file}");
        assert!(file.contains(TRACK), "{name}: {file}");
        assert!(
            stdout.contains("wrote 1 decision to meada.decisions.toml"),
            "{name}: {stdout}"
        );
    }
}

#[test]
fn a_response_with_no_model_id_is_recorded_under_the_configured_model() {
    let (web, _, _) = chain("decider_model_fallback");
    let content = answer("2", 0.7).to_string();
    let no_model =
        || Reply::ok(serde_json::json!({"choices": [{"message": {"content": content}}]}));
    let stub = Stub::start(vec![no_model(), no_model()]);

    let (_, stderr, ok) = meada(
        &web,
        &["decide", "--decider"],
        &chat(&stub),
        "",
        Some(&stub),
    );

    assert!(ok, "{stderr}");
    let file = written(&web).unwrap();
    assert_eq!(
        file.matches("by = \"local/test-model\"").count(),
        2,
        "{file}"
    );
}

#[test]
fn a_proposal_below_the_threshold_is_printed_as_a_suggestion_and_one_at_it_is_written() {
    let (web, _, _) = chain("decider_threshold");
    // 0.5 is the default threshold: the health check sits exactly on it, the tracking URL
    // just below.
    let stub = Stub::start(vec![
        Reply::chat("local/test-model", answer("2", 0.5)),
        Reply::chat("local/test-model", answer("1", 0.49)),
    ]);

    let (stdout, stderr, ok) = meada(
        &web,
        &["decide", "--decider"],
        &chat(&stub),
        "",
        Some(&stub),
    );

    assert!(ok, "{stderr}");
    assert_golden("chain-web.decider.chat.suggestion.en.txt", &stdout);
    let file = written(&web).unwrap();
    assert!(file.contains(HEALTH) && !file.contains(TRACK), "{file}");
}

#[test]
fn min_p_moves_the_threshold_and_values_outside_zero_to_one_are_errors() {
    let (web, _, _) = chain("decider_min_p");
    let stub = Stub::start(vec![
        Reply::chat("local/test-model", answer("2", 0.8)),
        Reply::chat("local/test-model", answer("1", 0.79)),
    ]);

    let mut args = vec!["decide", "--decider", "--min-p", "0.8"];
    let (stdout, stderr, ok) = meada(&web, &args, &chat(&stub), "", Some(&stub));
    assert!(ok, "{stderr}");
    assert!(stdout.contains("below --min-p 0.80"), "{stdout}");
    let file = written(&web).unwrap();
    assert!(file.contains(HEALTH) && !file.contains(TRACK), "{file}");

    // Out of range or not a number: an error before any request.
    let before = stub.requests().len();
    for value in ["1.1", "-0.1", "nan", "inf"] {
        args[3] = value;
        let (stdout, stderr, ok) = meada(&web, &args, &chat(&stub), "", Some(&stub));
        assert!(!ok && stdout.is_empty(), "{value}: {stdout}");
        assert!(
            stderr.starts_with("meada: `--min-p` must be a number from 0 to 1"),
            "{value}: {stderr}"
        );
    }
    assert_eq!(stub.requests().len(), before);
}

#[test]
fn dry_run_sends_and_prints_the_proposals_and_leaves_the_file_alone() {
    let (web, _, _) = chain("decider_dry");
    let stub = Stub::start(vec![
        Reply::chat("local/test-model", answer("2", 0.9)),
        Reply::chat("local/test-model", answer("1", 0.2)),
    ]);

    let (stdout, stderr, ok) = meada(
        &web,
        &["decide", "--decider", "--dry-run"],
        &chat(&stub),
        "",
        Some(&stub),
    );

    assert!(ok, "{stderr}");
    assert_eq!(stub.requests().len(), 2);
    assert!(
        stdout.contains("proposed: ") && stdout.contains("suggestion, not written: "),
        "{stdout}"
    );
    assert!(stdout.contains(&format!("call = \"{HEALTH}\"")), "{stdout}");
    assert!(!stdout.contains(&format!("call = \"{TRACK}\"")), "{stdout}");
    assert!(written(&web).is_none());
}

#[test]
fn a_key_reflected_in_a_success_answer_reaches_no_output_and_no_file() {
    let (web, _, _) = chain("decider_reflect");
    // The first answer names the key as its model id, which would be recorded; the second
    // has it as the probability, which a parse error would quote.
    let stub = Stub::start(vec![
        Reply::chat(KEY, answer("2", 0.9)),
        Reply::ok(serde_json::json!({
            "model": "local/test-model",
            "choices": [{"message": {"content": format!(r#"{{"choice": "1", "probability": "{KEY}"}}"#)}}],
        })),
    ]);

    // `meada` itself asserts that the key is in neither stream.
    let (stdout, _, ok) = meada(
        &web,
        &["decide", "--decider"],
        &chat(&stub),
        "",
        Some(&stub),
    );

    assert!(!ok);
    assert!(stdout.contains("<hidden>"), "{stdout}");
    let file = written(&web).unwrap();
    assert!(
        !file.contains(KEY) && file.contains("by = \"<hidden>\""),
        "{file}"
    );
}

#[test]
fn min_p_is_validated_with_list_and_prune_too() {
    let (web, _, _) = chain("decider_min_p_list");
    for mode in ["--list", "--prune"] {
        let args = ["decide", "--decider", mode, "--min-p", "7"];
        let (_, stderr, ok) = meada(&web, &args, &[], "", None);
        assert!(!ok, "{mode}");
        assert!(
            stderr.starts_with("meada: `--min-p` must be a number from 0 to 1"),
            "{mode}: {stderr}"
        );
    }
}

/// A `decisions` answer of type `choice`.
fn choice(choice: &str, confidence: f64, probabilities: serde_json::Value) -> serde_json::Value {
    serde_json::json!({
        "type": "choice",
        "choice": choice,
        "confidence": confidence,
        "probabilities": probabilities,
    })
}

/// A `decisions` response with `answers`, served by a dated id of the default model, with every
/// field the upstream documentation lists.
fn verdict(answers: serde_json::Value) -> serde_json::Value {
    serde_json::json!({
        "id": "gen-dec-stub",
        "model": "typesafe/jev-1.13-20260301",
        "provider": "TypeSafe",
        "answers": answers,
        "usage": {"input_tokens": 420, "output_tokens": 0, "cost": 0.0001},
    })
}

/// The health check picks the gateway (`2`), the tracking URL the catalog (`1`).
fn valid_answers() -> serde_json::Value {
    serde_json::json!({
        "item1": choice("2", 0.8, serde_json::json!({"1": 0.1, "2": 0.9})),
        "item2": choice("1", 0.4, serde_json::json!({"1": 0.62, "2": 0.38})),
    })
}

#[test]
fn decisions_asks_every_item_in_one_request_and_records_the_returned_model() {
    let (web, _, _) = chain("decider_decisions");
    let stub = Stub::start(vec![Reply::ok(verdict(valid_answers()))]);

    let args = ["decide", "--decider"];
    let (stdout, stderr, ok) = meada(&web, &args, &decisions(&stub), "", Some(&stub));

    assert!(ok, "{stderr}");
    assert_golden("chain-web.decider.decisions.en.txt", &stdout);
    let file = written(&web).expect("the file is written");
    assert_golden("chain-web.decisions.decisions.toml", &file);
    let requests = stub.requests();
    assert_eq!(requests.len(), 1);
    let request = &requests[0];
    assert_eq!(
        (request.method.as_str(), request.path.as_str()),
        ("POST", "/api/alpha/decisions")
    );
    let bearer = format!("Bearer {KEY}");
    assert_eq!(request.header("authorization"), Some(bearer.as_str()));
    let body = request.json();
    assert_eq!(
        body["provider"],
        serde_json::json!({"zdr": true, "data_collection": "deny"})
    );
    let names: Vec<_> = body["questions"].as_object().unwrap().keys().collect();
    assert_eq!(names, ["item1", "item2"]);
}

#[test]
fn a_decisions_suggestion_below_the_threshold_prints_its_confidence_too() {
    let (web, _, _) = chain("decider_decisions_min_p");
    let stub = Stub::start(vec![Reply::ok(verdict(valid_answers()))]);

    let args = ["decide", "--decider", "--min-p", "0.7"];
    let (stdout, stderr, ok) = meada(&web, &args, &decisions(&stub), "", Some(&stub));

    assert!(ok, "{stderr}");
    assert!(
        stdout.contains(&format!(
            "{TRACK}\nsuggestion, not written: example.com/chain/catalog POST /api/events \
             (p=0.62, confidence 0.40, below --min-p 0.70)\n"
        )),
        "{stdout}"
    );
    assert!(!written(&web).unwrap().contains(TRACK));
}

#[test]
fn a_provider_refusal_of_the_data_retention_preferences_writes_nothing_and_is_not_retried() {
    let (web, _, _) = chain("decider_refusal");
    // A retry would get the valid answer and write it.
    let refusal =
        r#"{"error": {"code": 404, "message": "No endpoints found matching your data policy"}}"#;
    let stub = Stub::start(vec![
        Reply::status(404, refusal),
        Reply::ok(verdict(valid_answers())),
    ]);

    let args = ["decide", "--decider"];
    let (stdout, stderr, ok) = meada(&web, &args, &decisions(&stub), "", Some(&stub));

    assert!(!ok);
    assert!(stderr.contains("2 item(s) failed"), "{stderr}");
    for item in [HEALTH, TRACK] {
        let report = format!(
            "{item}\nrequest failed: http status: 404: No endpoints found matching your data \
             policy\n"
        );
        assert!(stdout.contains(&report), "{stdout}");
    }
    assert!(written(&web).is_none());
    let requests = stub.requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(
        requests[0].json()["provider"],
        serde_json::json!({"zdr": true, "data_collection": "deny"})
    );
}

#[test]
fn a_key_escaped_in_an_error_message_reaches_no_output() {
    let (web, _, _) = chain("decider_escaped_key");
    let escaped = KEY.replace('&', "\\u0026");
    let body = format!(r#"{{"error": {{"code": 401, "message": "bad key {escaped}"}}}}"#);
    let stub = Stub::start(vec![Reply::status(401, &body)]);

    // `meada` itself asserts that the key is in neither stream.
    let args = ["decide", "--decider"];
    let (stdout, _, ok) = meada(&web, &args, &decisions(&stub), "", Some(&stub));

    assert!(!ok);
    assert!(
        stdout.contains("request failed: http status: 401: bad key <hidden>\n"),
        "{stdout}"
    );
}

#[test]
fn a_malformed_decisions_answer_fails_its_item_or_the_whole_response() {
    let item2 = || choice("1", 0.4, serde_json::json!({"1": 0.62, "2": 0.38}));
    // A bad answer for the first item, beside a valid one for the second.
    let one =
        |item1: serde_json::Value| verdict(serde_json::json!({"item1": item1, "item2": item2()}));
    let mut no_confidence = choice("2", 0.8, serde_json::json!({"1": 0.1, "2": 0.9}));
    no_confidence.as_object_mut().unwrap().remove("confidence");
    let mut rationale = choice("2", 0.8, serde_json::json!({"1": 0.1, "2": 0.9}));
    rationale["rationale"] = "because".into();
    let mut warnings = verdict(valid_answers());
    warnings["warnings"] = serde_json::json!([]);
    let mut extra = valid_answers();
    extra["item3"] = item2();
    let mut no_model = verdict(valid_answers());
    no_model.as_object_mut().unwrap().remove("model");
    let mut scored = choice("2", 0.8, serde_json::json!({"1": 0.1, "2": 0.9}));
    scored["type"] = "score".into();

    // (name, response, reason, whether the whole response fails)
    let cases = [
        (
            "sum below one",
            one(choice("2", 0.8, serde_json::json!({"1": 0.2, "2": 0.7}))),
            "sum to 0.9",
            false,
        ),
        (
            "key outside the candidates",
            one(choice("2", 0.8, serde_json::json!({"2": 0.9, "3": 0.1}))),
            "`3` is not an offered key",
            false,
        ),
        (
            "choice outside the candidates",
            one(choice("3", 0.8, serde_json::json!({"1": 0.5, "2": 0.5}))),
            "choice `3` is not one of the offered keys",
            false,
        ),
        (
            "probability outside zero to one",
            one(choice("2", 0.8, serde_json::json!({"1": -0.1, "2": 1.1}))),
            "probability -0.1 is not between 0 and 1",
            false,
        ),
        (
            "confidence outside zero to one",
            one(choice("2", 1.5, serde_json::json!({"1": 0.1, "2": 0.9}))),
            "confidence 1.5 is not between 0 and 1",
            false,
        ),
        (
            "no probability for the choice",
            one(choice("2", 0.8, serde_json::json!({"1": 1.0}))),
            "no probability for the choice `2`",
            false,
        ),
        ("not a choice", one(scored), "type `score`", false),
        (
            "no answer for an item",
            verdict(serde_json::json!({"item2": item2()})),
            "no answer",
            false,
        ),
        (
            "missing field",
            one(no_confidence),
            "missing field `confidence`",
            true,
        ),
        (
            "unknown field in an answer",
            one(rationale),
            "unknown field `rationale`",
            true,
        ),
        (
            "unknown field in the response",
            warnings,
            "unknown field `warnings`",
            true,
        ),
        (
            "answer to an unknown question",
            verdict(extra),
            "`item3`",
            true,
        ),
        ("no model", no_model, "missing field `model`", true),
    ];

    for (name, response, reason, whole) in cases {
        let (web, _, _) = chain(&format!("decider_bad_decisions_{}", name.replace(' ', "_")));
        let stub = Stub::start(vec![Reply::ok(response)]);

        let args = ["decide", "--decider"];
        let (stdout, stderr, ok) = meada(&web, &args, &decisions(&stub), "", Some(&stub));

        assert!(!ok, "{name}");
        let failing = if whole {
            &[HEALTH, TRACK][..]
        } else {
            &[HEALTH]
        };
        let summary = format!("{} item(s) failed", failing.len());
        assert!(stderr.contains(&summary), "{name}: {stderr}");
        for item in failing {
            let report = format!("{item}\ninvalid answer: ");
            let at = (stdout.find(&report)).unwrap_or_else(|| panic!("{name}: {stdout}"));
            let line = stdout[at + report.len()..].lines().next().unwrap();
            assert!(line.contains(reason), "{name}: {stdout}");
        }
        let file = written(&web);
        assert!(
            !file.as_deref().unwrap_or_default().contains(HEALTH),
            "{name}"
        );
        assert_eq!(
            file.is_some_and(|f| f.contains(TRACK)),
            !whole,
            "{name}: {stdout}"
        );
    }
}
