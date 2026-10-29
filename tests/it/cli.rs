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
/// `meada ui`, `meada postgres`) is absent from it.
#[cfg(not(any(
    feature = "snapshot",
    feature = "github-app",
    feature = "ui",
    feature = "postgres"
)))]
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

/// The identity provider's `[oidc]` errors (ADR 0016) under `pt_BR` and `en_US`: a `type` it
/// does not know, `[oidc.entra]` under another `type`, and an `overage` it does not take.
#[cfg(feature = "oidc")]
#[test]
fn oidc_type_errors_follow_lang() {
    use crate::manifest::{MANIFEST, chain_manifest};
    let dir = chain_manifest("cli_oidc_type");
    let head = "index_dir = \"../indexes\"\n\n[oidc]\nissuer = \"https://id.example\"\npublic_url = \
                \"https://meada.example\"\nidentities_claim = \"r\"\n";
    let at = "meada: ops/meada.workspace.toml";
    for (body, en, pt) in [
        (
            "type = \"azure\"\n",
            "7: `oidc.type`: must be one of `generic`, `entra`, `okta` or `keycloak`",
            "7: `oidc.type`: deve ser um de `generic`, `entra`, `okta` ou `keycloak`",
        ),
        (
            "type = \"okta\"\n[oidc.entra]\n",
            "8: `oidc.entra`: needs `oidc.type = \"entra\"`",
            "8: `oidc.entra`: exige `oidc.type = \"entra\"`",
        ),
        (
            "type = \"entra\"\n[oidc.entra]\noverage = \"directory\"\n",
            "9: `oidc.entra.overage`: must be `ignore`",
            "9: `oidc.entra.overage`: deve ser `ignore`",
        ),
    ] {
        fs::write(dir.join(MANIFEST), format!("{head}{body}")).unwrap();
        for (lang, what) in [("en_US.UTF-8", en), ("pt_BR.UTF-8", pt)] {
            let refused = (String::new(), format!("{at}:{what}\n"), Some(1));
            let args = ["index", "--workspace", MANIFEST];
            assert_eq!(run_lang(&dir, lang, &args), refused, "{lang} {body}");
        }
    }
}

/// An entry's `owner` (ADR 0015) under `pt_BR` and `en_US`: each rule it breaks, and the static
/// manifest that holds one at all.
#[test]
fn owner_errors_follow_lang() {
    use crate::manifest::{MANIFEST, chain_manifest};
    let dir = chain_manifest("cli_owner");
    let head = "index_dir = \"../indexes\"\n\n[[repository]]\nrepo = \"example.com/chain/web\"\n\
                path = \"../chain-web\"\n";
    let manifest = "meada: ops/meada.workspace.toml:6: `repository.owner`";
    let long = "g".repeat(257);
    let cases = [
        (
            "owner = 1\n".to_string(),
            "must be one string, the name of one group",
            "deve ser uma única string, o nome de um grupo",
        ),
        (
            "owner = [\"a\"]\n".into(),
            "must be one string, the name of one group",
            "deve ser uma única string, o nome de um grupo",
        ),
        ("owner = \"\"\n".into(), "is empty", "está vazio"),
        (
            format!("owner = \"{long}\"\n"),
            "is longer than 256 characters",
            "tem mais de 256 caracteres",
        ),
        (
            "owner = \"te\\u0007am\"\n".into(),
            "holds a control character",
            "contém um caractere de controle",
        ),
        (
            "owner = \"team \"\n".into(),
            "has whitespace at either end",
            "tem espaço em branco numa das pontas",
        ),
        (
            "owner = \"team\"\n".into(),
            "an owner belongs to the entries of a workspace version, which a static manifest \
             has none of: use [workspace] source = \"store\" and `meada workspace import`",
            "um dono pertence às entradas de uma versão do workspace, que um manifest estático \
             não tem: use [workspace] source = \"store\" e `meada workspace import`",
        ),
    ];
    for (owner, en, pt) in cases {
        fs::write(dir.join(MANIFEST), format!("{head}{owner}")).unwrap();
        for (lang, what) in [("en_US.UTF-8", en), ("pt_BR.UTF-8", pt)] {
            let refused = (String::new(), format!("{manifest}: {what}\n"), Some(1));
            let args = ["index", "--workspace", MANIFEST];
            assert_eq!(run_lang(&dir, lang, &args), refused, "{lang} {owner}");
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
                "{manifest}: [workspace] source = \"store\": no workspace version pulled in \
                 ops/../indexes/workspace: run `meada workspace pull` first\n"
            ),
            format!(
                "{manifest}: [workspace] source = \"store\": nenhuma versão do workspace \
                 baixada em ops/../indexes/workspace: rode `meada workspace pull` antes\n"
            ),
        ),
        (
            bootstrap.clone(),
            vec!["mcp", "--listen", "127.0.0.1:0", "--workspace", MANIFEST],
            format!(
                "{manifest}: [workspace] source = \"store\": a managed workspace's nodes serve \
                 generations: run meada mcp --listen with --generations\n"
            ),
            // The MCP surface is English whatever `LANG` says (ADR 0008).
            format!(
                "{manifest}: [workspace] source = \"store\": a managed workspace's nodes serve \
                 generations: run meada mcp --listen with --generations\n"
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
            entries.clone(),
            vec!["workspace", "pull", "--workspace", MANIFEST],
            format!(
                "{manifest}: no [generations] section: workspace versions need [snapshot] and \
                 [generations]\n"
            ),
            format!(
                "{manifest}: sem seção [generations]: versões do workspace exigem [snapshot] e \
                 [generations]\n"
            ),
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
        let apply = |id| ["workspace", "apply", id, "--workspace", MANIFEST];
        let malformed = malformed.replace("generation id", "workspace version id");
        let malformed = malformed.replace("id de geração de 16", "id de versão do workspace de 16");
        let malformed_id = run_in(&dir, lang, &apply("000000000000000A"));
        assert_eq!(malformed_id, refused(&malformed), "{lang} apply");
        let args = apply("0000000000000000");
        assert_eq!(run_in(&dir, lang, &args), refused(section), "{lang} apply");
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

/// A C# file whose qualified names pass the pack's budget is dropped, and `status` names it
/// and the limit.
#[test]
fn status_reports_a_file_over_a_pack_budget_follow_lang() {
    let repo = fixture_repo("cli_budget", "ts-basic", None);
    let mut source = format!("namespace N{};\npublic class C {{\n", "a".repeat(4096));
    for n in 0..2000 {
        source += &format!("public void M{n}() {{ F(); }}\n");
    }
    fs::write(repo.join("Broad.cs"), source + "}\n").unwrap();
    // A file a pack leaves unbuilt on purpose is not this drop.
    fs::write(
        repo.join("cgo.go"),
        "package x\n\nimport \"C\"\n\nfunc F() {}\n",
    )
    .unwrap();
    meada(&repo, "en_US.UTF-8", "index");
    for (lang, name) in LANGS {
        let status = meada(&repo, lang, "status");
        assert!(!status.contains("cgo.go"), "{status}");
        assert_golden(&format!("cli.status-budget.{name}.txt"), &status);
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

/// `meada workspace apply` and `pull` against the services, under `pt_BR` and `en_US`: a
/// version the store does not hold, no current version, a pointer whose fence is not above the
/// current file's, and a pull that finds its version current.
#[cfg(feature = "refresh-lock")]
#[test]
fn workspace_apply_and_pull_refusals_follow_lang() {
    use crate::generations::{admin, import, prefix, send, version_key, workspace_pointer};
    use crate::manifest::MANIFEST;
    use crate::refresh_lock::{fleet, run_in};
    let Some(fleet) = fleet("cli_workspace_pull") else {
        return;
    };
    let dir = &fleet.dir;
    let pointer = workspace_pointer(fleet.case);
    let fence_key = format!("{{{}}}:fence", prefix(fleet.case));
    let refused = |stderr: String| (String::new(), stderr, Some(1));
    let pull = ["workspace", "pull", "--workspace", MANIFEST];
    let zero = "0000000000000000";
    let apply = ["workspace", "apply", zero, "--workspace", MANIFEST];
    let missing = version_key(zero);
    for (lang, none, unknown) in [
        (
            "en_US.UTF-8",
            format!(
                "meada: {pointer}: no workspace version is current: apply one with `meada \
                 workspace apply`\n"
            ),
            format!("meada: {missing}: no such workspace version in the store\n"),
        ),
        (
            "pt_BR.UTF-8",
            format!(
                "meada: {pointer}: nenhuma versão do workspace é a atual: aplique uma com \
                 `meada workspace apply`\n"
            ),
            format!("meada: {missing}: essa versão do workspace não está no store\n"),
        ),
    ] {
        assert_eq!(run_in(dir, lang, &pull), refused(none), "{lang}");
        assert_eq!(run_in(dir, lang, &apply), refused(unknown), "{lang}");
    }

    let (v1, _) = import(dir, MANIFEST, "one", "alice");
    let mut valkey = admin(&fleet.valkey);
    send(&mut valkey, &["HSET", &pointer, "id", &v1, "fence", "2"]);
    assert_eq!(run_in(dir, "en_US.UTF-8", &pull).2, Some(0));
    let current = (
        format!("{v1}\n"),
        format!("meada: {v1}: já é a versão atual do workspace: nada mudou\n"),
        Some(0),
    );
    assert_eq!(run_in(dir, "pt_BR.UTF-8", &pull), current);
    send(&mut valkey, &["HSET", &pointer, "fence", "1"]);
    let file = "ops/../indexes/workspace/current";
    for (lang, expected) in [
        (
            "en_US.UTF-8",
            format!(
                "meada: {pointer}: workspace version {v1} has fence 1, not above fence 2 in \
                 {file}: not swapped; raise {fence_key} with meada snapshot fence recover \
                 --at-least 2 and apply again, or delete {file}\n"
            ),
        ),
        (
            "pt_BR.UTF-8",
            format!(
                "meada: {pointer}: a versão do workspace {v1} tem fence 1, não maior que o fence \
                 2 em {file}: não trocada; eleve {fence_key} com meada snapshot fence recover \
                 --at-least 2 e aplique de novo, ou apague {file}\n"
            ),
        ),
    ] {
        assert_eq!(run_in(dir, lang, &pull), refused(expected), "{lang}");
    }
}

/// `[postgres]` in a build without its feature, under `pt_BR` and `en_US`.
#[cfg(not(feature = "postgres"))]
#[test]
fn the_postgres_section_without_its_feature_follows_lang() {
    use crate::manifest::{MANIFEST, chain_manifest};
    let dir = chain_manifest("cli_postgres_feature");
    let toml = "index_dir = \"../indexes\"\n\n[postgres]\nschema = \"meada\"\n";
    fs::write(dir.join(MANIFEST), toml).unwrap();
    for (lang, expected) in [
        (
            "en_US.UTF-8",
            "meada: ops/meada.workspace.toml:3: [postgres] needs a Meada built with the postgres \
             feature\n",
        ),
        (
            "pt_BR.UTF-8",
            "meada: ops/meada.workspace.toml:3: [postgres] exige um Meada compilado com a feature \
             postgres\n",
        ),
    ] {
        let refused = (String::new(), expected.to_string(), Some(1));
        let args = ["status", "--workspace", MANIFEST];
        assert_eq!(run_lang(&dir, lang, &args), refused, "{lang}");
    }
    let (_, stderr, code) = run_lang(&dir, "en_US.UTF-8", &["postgres", "migrate"]);
    assert_eq!(code, Some(2));
    assert!(stderr.contains("unrecognized subcommand"), "{stderr}");
}

/// With `postgres`, under `pt_BR` and `en_US`: a `[postgres]` key error, the URL refusals and,
/// against the local Postgres, both version refusals.
#[cfg(feature = "postgres")]
#[test]
fn postgres_refusals_follow_lang() {
    use crate::postgres::{Admin, MIGRATE, manifest, meada, schema, service};
    let (en, pt) = ("en_US.UTF-8", "pt_BR.UTF-8");
    let dir = manifest("cli_postgres_key", "schema = \"Meada\"\n");
    for (lang, expected) in [
        (
            en,
            "meada: ops/meada.workspace.toml:4: `postgres.schema`: must be 1 to 63 lowercase \
             letters, digits or `_`, starting with a letter\n",
        ),
        (
            pt,
            "meada: ops/meada.workspace.toml:4: `postgres.schema`: deve ter de 1 a 63 letras \
             minúsculas, dígitos ou `_`, começando por uma letra\n",
        ),
    ] {
        assert_eq!(meada(&dir, lang, &[], &MIGRATE).1, expected, "{lang}");
    }
    let dir = manifest("cli_postgres_url", "schema = \"meada\"\n");
    let query = [(
        "MEADA_POSTGRES_URL",
        "postgres://db.example.internal/m?sslmode=disable",
    )];
    let socket = [("MEADA_POSTGRES_URL", "postgres://%2Fvar%2Frun/m")];
    for (env, lang, expected) in [
        (
            &[][..],
            en,
            "`postgres.url_env`: the variable MEADA_POSTGRES_URL is not set",
        ),
        (
            &[][..],
            pt,
            "`postgres.url_env`: a variável MEADA_POSTGRES_URL não está definida",
        ),
        (
            &query[..],
            en,
            "`postgres.url_env`: holds a URL with a query: no parameter is accepted, TLS is \
             decided by the host",
        ),
        (
            &query[..],
            pt,
            "`postgres.url_env`: traz uma URL com query: nenhum parâmetro é aceito, o TLS é \
             decidido pelo host",
        ),
        (
            &socket[..],
            en,
            "`postgres.url_env`: does not hold a postgres:// or postgresql:// URL with a TCP host",
        ),
        (
            &socket[..],
            pt,
            "`postgres.url_env`: não traz uma URL postgres:// ou postgresql:// com um host TCP",
        ),
    ] {
        let (_, stderr, code) = meada(&dir, lang, env, &MIGRATE);
        assert_eq!(
            (stderr, code),
            (format!("meada: {expected}\n"), 1),
            "{lang}"
        );
    }

    let Some(url) = service() else { return };
    let schema = schema("cli_version");
    let admin = Admin::new(&url);
    admin.run(&format!("DROP SCHEMA IF EXISTS \"{schema}\" CASCADE"));
    let dir = manifest("cli_postgres_version", &format!("schema = \"{schema}\"\n"));
    let at = |cap| {
        vec![
            ("MEADA_POSTGRES_URL", url.as_str()),
            ("MEADA_POSTGRES_VERSION_MAX", cap),
        ]
    };
    let status = ["status", "--workspace", crate::manifest::MANIFEST];
    let line = |lang, cap| {
        let (status, _, _) = meada(&dir, lang, &at(cap), &status);
        status
            .lines()
            .find(|l| l.starts_with("postgres: "))
            .unwrap()
            .to_string()
    };
    assert_eq!(
        line(en, "9"),
        format!(
            "postgres: unreachable (the schema {schema} is at version 0, below this Meada's 1: \
             run meada postgres migrate)"
        )
    );
    assert_eq!(
        line(pt, "9"),
        format!(
            "postgres: inalcançável (o schema {schema} está na versão 0, abaixo da 1 deste Meada: \
             rode meada postgres migrate)"
        )
    );
    assert_eq!(meada(&dir, en, &at("9"), &MIGRATE).0, "1\n");
    for (lang, expected) in [
        (
            en,
            format!("the schema {schema} is at version 1, above this Meada's 0: upgrade Meada"),
        ),
        (
            pt,
            format!("o schema {schema} está na versão 1, acima da 0 deste Meada: atualize o Meada"),
        ),
    ] {
        let (_, stderr, code) = meada(&dir, lang, &at("0"), &MIGRATE);
        assert_eq!(
            (stderr, code),
            (format!("meada: {expected}\n"), 1),
            "{lang}"
        );
    }
    admin.run(&format!("DROP SCHEMA \"{schema}\" CASCADE"));
}
