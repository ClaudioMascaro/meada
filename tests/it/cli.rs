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

/// `meada <args>` in `dir` under `lang`: stdout, stderr and the exit code.
fn run_lang(dir: &Path, lang: &str, args: &[&str]) -> (String, String, Option<i32>) {
    let out = Command::new(env!("CARGO_BIN_EXE_meada"))
        .current_dir(dir)
        .args(args)
        .env("LANG", lang)
        .output()
        .expect("meada runs");
    let text = |bytes: &[u8]| String::from_utf8_lossy(bytes).into_owned();
    (text(&out.stdout), text(&out.stderr), out.status.code())
}

/// `[workspace]`'s errors under `pt_BR` and `en_US`: an unknown key, and `source = "store"`
/// in a build without `generation-pointer`.
#[test]
fn workspace_section_errors_follow_lang() {
    use crate::manifest::{MANIFEST, chain_manifest};
    let dir = chain_manifest("cli_workspace_section");
    let head = "index_dir = \"../indexes\"\n\n[workspace]\n";
    let mut cases = vec![(
        "mode = 1\n",
        "meada: ops/meada.workspace.toml:4: `workspace.mode` is not a meada.workspace.toml key\n",
        "meada: ops/meada.workspace.toml:4: `workspace.mode` não é uma chave do \
         meada.workspace.toml\n",
    )];
    if !cfg!(feature = "generation-pointer") {
        cases.push((
            "source = \"store\"\n",
            "meada: ops/meada.workspace.toml:4: `workspace.source`: \"store\" needs a Meada built \
             with the generation-pointer feature\n",
            "meada: ops/meada.workspace.toml:4: `workspace.source`: \"store\" exige um Meada \
             compilado com a feature generation-pointer\n",
        ));
    }
    for (section, en, pt) in cases {
        fs::write(dir.join(MANIFEST), format!("{head}{section}")).unwrap();
        for (lang, expected) in [("en_US.UTF-8", en), ("pt_BR.UTF-8", pt)] {
            let refused = (String::new(), expected.to_string(), Some(1));
            let args = ["index", "--workspace", MANIFEST];
            assert_eq!(run_lang(&dir, lang, &args), refused, "{lang} {section}");
        }
    }
}

/// With `generation-pointer`, under `pt_BR` and `en_US`: `[workspace] source = "store"` without
/// its sections and beside `[[repository]]`, a command that reads entries refusing it, and the
/// refusals of `meada workspace` that need no service (a malformed id, checked first, and a
/// bootstrap manifest without `[generations]`).
#[cfg(feature = "generation-pointer")]
#[test]
fn workspace_refusals_follow_lang() {
    use crate::manifest::{MANIFEST, chain_manifest, line_of};
    let dir = chain_manifest("cli_workspace");
    let entries = fs::read_to_string(dir.join(MANIFEST)).unwrap();
    let sections = format!(
        "{}\n[valkey]\nurl = \"redis://127.0.0.1:6390\"\n\n[generations]\nkeep = 3\n",
        crate::snapshot::section("http://127.0.0.1:9", "nowhere")
    );
    let store = "\n[workspace]\nsource = \"store\"\n";
    let manifest = "meada: ops/meada.workspace.toml";
    let no_sections = format!("index_dir = \"../indexes\"\n{store}");
    let beside = format!("{entries}{sections}{store}");
    let bootstrap = format!("index_dir = \"../indexes\"\n{sections}{store}");
    let cases = [
        (
            no_sections,
            vec!["index", "--workspace", MANIFEST],
            format!("{manifest}:3: [workspace] needs [snapshot] and [generations]\n"),
            format!("{manifest}:3: [workspace] exige [snapshot] e [generations]\n"),
        ),
        (
            beside.clone(),
            vec!["index", "--workspace", MANIFEST],
            format!(
                "{manifest}:{}: `repository`: [[repository]] entries beside [workspace] source = \
                 \"store\": the repositories come from the current workspace version\n",
                line_of(&beside, "[[repository]]")
            ),
            format!(
                "{manifest}:{}: `repository`: entradas [[repository]] ao lado de [workspace] \
                 source = \"store\": os repositórios vêm da versão atual do workspace\n",
                line_of(&beside, "[[repository]]")
            ),
        ),
        (
            bootstrap.clone(),
            vec!["index", "--workspace", MANIFEST],
            format!(
                "{manifest}: [workspace] source = \"store\": this command reads [[repository]] \
                 entries, which only source = \"file\" lists\n"
            ),
            format!(
                "{manifest}: [workspace] source = \"store\": este comando lê entradas \
                 [[repository]], que só source = \"file\" lista\n"
            ),
        ),
        (
            bootstrap,
            vec![
                "workspace",
                "show",
                "0000000000000ABC",
                "--workspace",
                MANIFEST,
            ],
            "meada: 0000000000000ABC: not a workspace version id of 16 lowercase hex \
             characters\n"
                .into(),
            "meada: 0000000000000ABC: não é um id de versão do workspace de 16 caracteres \
             hexadecimais minúsculos\n"
                .into(),
        ),
        (
            entries,
            vec!["workspace", "list", "--workspace", MANIFEST],
            format!(
                "{manifest}: no [generations] section: workspace versions need [snapshot] and \
                 [generations]\n"
            ),
            format!(
                "{manifest}: sem seção [generations]: versões do workspace exigem [snapshot] e \
                 [generations]\n"
            ),
        ),
    ];
    fs::write(dir.join("ops/s3-secret"), "secret\n").unwrap();
    for (toml, args, en, pt) in cases {
        fs::write(dir.join(MANIFEST), &toml).unwrap();
        for (lang, expected) in [("en_US.UTF-8", en), ("pt_BR.UTF-8", pt)] {
            let refused = (String::new(), expected, Some(1));
            assert_eq!(run_lang(&dir, lang, &args), refused, "{lang} {args:?}");
        }
    }
}

/// The operations' refusals that need no service, under `pt_BR` and `en_US`: a malformed id
/// (checked first) and a manifest without `[refresh_lock]`, for `pointer set`, `retain` and
/// `drop` and `fence recover`.
#[cfg(feature = "refresh-lock")]
#[test]
fn operation_refusals_follow_lang() {
    use crate::manifest::{MANIFEST, chain_manifest};
    use crate::refresh_lock::run_in;
    let dir = chain_manifest("cli_operations");
    let pointer = |op, id| ["snapshot", "pointer", op, id, "--workspace", MANIFEST];
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
        for op in ["set", "retain", "drop"] {
            let malformed_id = run_in(&dir, lang, &pointer(op, "000000000000000A"));
            assert_eq!(malformed_id, refused(malformed), "{lang} {op}");
            let args = pointer(op, "0000000000000000");
            assert_eq!(run_in(&dir, lang, &args), refused(section), "{lang} {op}");
        }
        assert_eq!(run_in(&dir, lang, &recover), refused(section), "{lang}");
    }
}

/// The operations' refusals against Valkey, under `pt_BR` and `en_US`: a generation not
/// retained (`pointer set` and `drop`), the current one dropped, a full kept set, a lock whose
/// holder is named, and one with no holder (taken by an older binary).
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
    let (current, kept) = (key(&fleet, "current"), key(&fleet, "kept"));
    let drop = |id| ["snapshot", "pointer", "drop", id, "--workspace", MANIFEST];
    let retain = |id| ["snapshot", "pointer", "retain", id, "--workspace", MANIFEST];
    let filler: Vec<String> = (0..8).map(|n| format!("{n:016x}")).collect();
    let sadd: Vec<&str> = ["SADD", kept.as_str()]
        .into_iter()
        .chain(filler.iter().map(String::as_str))
        .collect();
    send(&mut admin(&fleet.valkey), &sadd);
    let operations = [
        (
            "en_US.UTF-8",
            format!("meada: {retained}: generation {zero} is not retained: nothing dropped\n"),
            format!("meada: {current}: generation {g1} is the current one: nothing dropped\n"),
            format!(
                "meada: {kept}: already keeps 8 generations, the most it may: drop one first \
                 (meada snapshot pointer drop)\n"
            ),
        ),
        (
            "pt_BR.UTF-8",
            format!("meada: {retained}: a geração {zero} não está retida: nada removido\n"),
            format!("meada: {current}: a geração {g1} é a atual: nada removido\n"),
            format!(
                "meada: {kept}: já mantém 8 gerações, o máximo permitido: remova uma antes \
                 (meada snapshot pointer drop)\n"
            ),
        ),
    ];
    for (lang, outside, is_current, full) in operations {
        assert_eq!(run_in(dir, lang, &drop(zero)), refused(outside), "{lang}");
        assert_eq!(run_in(dir, lang, &drop(&g1)), refused(is_current), "{lang}");
        assert_eq!(run_in(dir, lang, &retain(&g1)), refused(full), "{lang}");
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

/// `[snapshot] node_name`'s error under `pt_BR` and `en_US`.
#[cfg(feature = "snapshot")]
#[test]
fn a_bad_node_name_follows_lang() {
    use crate::manifest::{MANIFEST, chain_manifest};
    let dir = chain_manifest("cli_node_name");
    let snapshot = crate::snapshot::section("http://127.0.0.1:9", "b-1");
    let toml = format!("index_dir = \"../indexes\"\n{snapshot}node_name = \"Upper\"\n");
    fs::write(dir.join(MANIFEST), &toml).unwrap();
    let line = toml.matches('\n').count();
    let manifest = "meada: ops/meada.workspace.toml";
    for (lang, expected) in [
        (
            "en_US.UTF-8",
            "must be 1 to 63 lowercase letters, digits, `.` and `-`",
        ),
        (
            "pt_BR.UTF-8",
            "deve ter de 1 a 63 letras minúsculas, dígitos, `.` e `-`",
        ),
    ] {
        let expected = format!("{manifest}:{line}: `snapshot.node_name`: {expected}\n");
        let refused = (String::new(), expected, Some(1));
        let args = ["index", "--workspace", MANIFEST];
        assert_eq!(run_lang(&dir, lang, &args), refused, "{lang}");
    }
}
