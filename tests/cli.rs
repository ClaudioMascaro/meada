//! The built binary's messages follow `LANG` (ADR 0008).

mod common;

use std::fs;
use std::path::Path;
use std::process::Command;

use common::{assert_golden, fixture_repo};

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
    fs::write(repo.join("meada.toml"), "[[resource]]\nkind = \"db\"\n").unwrap();
    for (lang, name) in LANGS {
        assert_golden(
            &format!("cli.config.{name}.txt"),
            &meada(&repo, lang, "index"),
        );
    }
}

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
