//! The built binary's messages follow `LANG` (ADR 0008).

use crate::common;

use std::fs;
use std::path::Path;
use std::process::Command;

use common::{
    assert_golden, chain, decisions, documented_chain, drift_chain, fixture_repo, loose_chain,
};

/// Runs `meada <arg>` in `repo` under `lang`; returns stdout and stderr, with the repository's
/// path replaced by `<repo>`.
fn meada(repo: &Path, lang: &str, arg: &str) -> String {
    let out = Command::new(env!("CARGO_BIN_EXE_meada"))
        .current_dir(repo)
        .arg(arg)
        .env("LANG", lang)
        .output()
        .expect("meada runs");
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    text.replace(repo.to_str().unwrap(), "<repo>")
}

/// `LANG` values and the golden suffix each one gets; the last one falls back to English.
const LANGS: [(&str, &str); 3] = [
    ("pt_BR.UTF-8", "pt"),
    ("en_US.UTF-8", "en"),
    ("de_DE.UTF-8", "fallback"),
];

#[test]
fn status_follows_lang() {
    let repo = fixture_repo(
        "cli_status",
        "ts-basic",
        Some("https://example.com/acme/web.git"),
    );
    fs::write(
        repo.join("meada.toml"),
        "[[peer]]\nrepo = \"example.com/acme/api\"\n",
    )
    .unwrap();
    for (lang, name) in LANGS {
        assert_golden(
            &format!("cli.status.{name}.txt"),
            &meada(&repo, lang, "status"),
        );
    }
}

#[test]
fn config_errors_follow_lang() {
    let repo = fixture_repo(
        "cli_config",
        "ts-basic",
        Some("https://example.com/acme/web.git"),
    );
    fs::write(
        repo.join("meada.toml"),
        "[serve]\ndomains = [\"openapi.yaml\"]\n",
    )
    .unwrap();
    for (lang, name) in LANGS {
        assert_golden(
            &format!("cli.config.{name}.txt"),
            &meada(&repo, lang, "index"),
        );
    }
}

/// The default build's help: a feature's subcommand (`meada snapshot`, `meada github-app`,
/// `meada ui`) is absent from it.
#[cfg(not(any(feature = "snapshot", feature = "github-app", feature = "ui")))]
#[test]
fn help_follows_lang() {
    let repo = fixture_repo("cli_help", "ts-basic", None);
    for (lang, name) in &LANGS[..2] {
        assert_golden(
            &format!("cli.help.{name}.txt"),
            &meada(&repo, lang, "--help"),
        );
    }
}

/// `meada ui` exists only in a build with the `ui` feature (ADR 0014).
#[cfg(not(feature = "ui"))]
#[test]
fn the_console_subcommand_is_absent_without_its_feature() {
    let out = Command::new(env!("CARGO_BIN_EXE_meada"))
        .args(["ui", "--listen", "0", "--workspace", "meada.workspace.toml"])
        .output()
        .expect("meada runs");
    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("unrecognized subcommand 'ui'"), "{stderr}");
}

/// Without the `refresh-lock` feature, `meada snapshot pointer` and `meada snapshot fence` do
/// not exist.
#[cfg(not(feature = "refresh-lock"))]
#[test]
fn the_operations_need_the_refresh_lock_feature() {
    let repo = fixture_repo("cli_no_operations", "ts-basic", None);
    for command in ["pointer", "fence"] {
        let out = Command::new(env!("CARGO_BIN_EXE_meada"))
            .current_dir(&repo)
            .args(["snapshot", command, "--help"])
            .output()
            .expect("meada runs");
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert_eq!(out.status.code(), Some(2), "{command}: {stderr}");
        assert!(stderr.contains("unrecognized subcommand"), "{stderr}");
    }
}

/// The operations' refusals that need no service, under `pt_BR` and `en_US`: a malformed id
/// (checked first) and a manifest without `[refresh_lock]`.
#[cfg(feature = "refresh-lock")]
#[test]
fn operation_refusals_follow_lang() {
    use crate::manifest::{MANIFEST, chain_manifest};
    use crate::refresh_lock::run_in;
    let dir = chain_manifest("cli_operations");
    let set = |id| ["snapshot", "pointer", "set", id, "--workspace", MANIFEST];
    let recover = ["snapshot", "fence", "recover", "--workspace", MANIFEST];
    let refused = |stderr: &str| (String::new(), stderr.to_string(), Some(1));
    for (lang, malformed, section) in [
        (
            "en_US.UTF-8",
            "meada: 000000000000000A: not a generation id of 16 lowercase hex characters\n",
            "meada: ops/meada.workspace.toml: no [refresh_lock] section: the lock has no TTL\n",
        ),
        (
            "pt_BR.UTF-8",
            "meada: 000000000000000A: não é um id de geração de 16 caracteres hexadecimais \
             minúsculos\n",
            "meada: ops/meada.workspace.toml: sem seção [refresh_lock]: o lock não tem TTL\n",
        ),
    ] {
        let malformed_id = run_in(&dir, lang, &set("000000000000000A"));
        assert_eq!(malformed_id, refused(malformed), "{lang}");
        for args in [&set("0000000000000000")[..], &recover] {
            assert_eq!(
                run_in(&dir, lang, args),
                refused(section),
                "{lang} {args:?}"
            );
        }
    }
}

/// The operations' refusals against Valkey, under `pt_BR` and `en_US`: a generation not
/// retained, a lock whose holder is named, and one with no holder (taken by an older binary).
#[cfg(feature = "refresh-lock")]
#[test]
fn operation_refusals_with_valkey_follow_lang() {
    use crate::generations::{admin, send};
    use crate::manifest::MANIFEST;
    use crate::refresh_lock::{fleet, key, locked_publish, run_in};
    let Some(fleet) = fleet("cli_operations_held") else {
        return;
    };
    let dir = &fleet.dir;
    let g1 = locked_publish(dir);
    let (lock, holder) = (key(&fleet, "lock"), key(&fleet, "holder"));
    let retained = key(&fleet, "retained");
    let set = |id| ["snapshot", "pointer", "set", id, "--workspace", MANIFEST];
    let recover = ["snapshot", "fence", "recover", "--workspace", MANIFEST];
    let refused = |stderr: String| (String::new(), stderr, Some(1));
    let zero = "0000000000000000";
    let not_retained = [
        (
            "en_US.UTF-8",
            format!(
                "meada: {retained}: generation {zero} is not retained: the pointer did not move\n"
            ),
        ),
        (
            "pt_BR.UTF-8",
            format!(
                "meada: {retained}: a geração {zero} não está retida: o ponteiro não se moveu\n"
            ),
        ),
    ];
    for (lang, expected) in not_retained {
        assert_eq!(run_in(dir, lang, &set(zero)), refused(expected), "{lang}");
    }

    let mut valkey = admin(&fleet.valkey);
    send(
        &mut valkey,
        &["SET", &lock, "0123456789abcdef0123456789abcdef"],
    );
    let (host, started, holds) = (
        "node-0",
        "2026-10-22T12:00:00Z",
        "pointer set cli:ana@node-0",
    );
    let fields = [
        "host", host, "pid", "42", "started", started, "holds", holds,
    ];
    let held = |valkey: &mut redis::Connection, named: bool| {
        send(valkey, &["DEL", &holder]);
        if named {
            let hset: Vec<&str> = ["HSET", holder.as_str()]
                .into_iter()
                .chain(fields)
                .collect();
            send(valkey, &hset);
        }
    };
    let cases = [
        (
            true,
            "en_US.UTF-8",
            format!(
                "meada: {lock}: held by host {host}, process 42, since {started}, for {holds}: nothing run\n"
            ),
        ),
        (
            true,
            "pt_BR.UTF-8",
            format!(
                "meada: {lock}: em posse do host {host}, processo 42, desde {started}, para {holds}: nada executado\n"
            ),
        ),
        (
            false,
            "en_US.UTF-8",
            format!("meada: {lock}: held by an unknown holder: nothing run\n"),
        ),
        (
            false,
            "pt_BR.UTF-8",
            format!("meada: {lock}: em posse de um detentor desconhecido: nada executado\n"),
        ),
    ];
    for (named, lang, expected) in cases {
        held(&mut valkey, named);
        for args in [&set(&g1)[..], &recover] {
            let out = run_in(dir, lang, args);
            assert_eq!(out, refused(expected.clone()), "{lang} {args:?}");
        }
    }
    send(&mut valkey, &["DEL", &lock, &holder]);
}

#[test]
fn status_reports_stale_and_waiting_decisions_follow_lang() {
    let (web, _, _) = chain("cli_stale");
    decisions(&web, "chain-web-missing-peer");
    let toml = web.join("meada.toml");
    let peer = "\n[[peer]]\nrepo = \"example.com/chain/billing\"\n";
    fs::write(&toml, fs::read_to_string(&toml).unwrap() + peer).unwrap();
    // A third decision, for a call that does not exist, is stale.
    let file = web.join("meada.decisions.toml");
    let gone = "\n[[decision]]\nkind = \"dynamic-url\"\n\
                call = \"example.com/chain/web::src/status.ts::gone::fetch#0\"\n\
                target = \"example.com/chain/catalog::http::POST::/api/events\"\nby = \"human\"\n";
    fs::write(&file, fs::read_to_string(&file).unwrap() + gone).unwrap();
    meada(&web, "en_US.UTF-8", "index");
    for (lang, name) in LANGS {
        assert_golden(
            &format!("cli.status-stale.{name}.txt"),
            &meada(&web, lang, "status"),
        );
    }
}

#[test]
fn status_reports_loose_resources_follow_lang() {
    let gateway = loose_chain("cli_resources");
    meada(&gateway, "en_US.UTF-8", "index");
    for (lang, name) in LANGS {
        assert_golden(
            &format!("cli.status-resources.{name}.txt"),
            &meada(&gateway, lang, "status"),
        );
    }
}

#[test]
fn status_reports_documents_follow_lang() {
    let (_, gateway, _) = documented_chain("cli_documents");
    meada(&gateway, "en_US.UTF-8", "index");
    for (lang, name) in LANGS {
        assert_golden(
            &format!("cli.status-documents.{name}.txt"),
            &meada(&gateway, lang, "status"),
        );
    }
}

#[test]
fn status_reports_drift_follow_lang() {
    let (web, _, _) = drift_chain("cli_drift");
    meada(&web, "en_US.UTF-8", "index");
    for (lang, name) in LANGS {
        assert_golden(
            &format!("cli.status-drift.{name}.txt"),
            &meada(&web, lang, "status"),
        );
    }
}
