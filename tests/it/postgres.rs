//! The data layer (ADR 0016, feature `postgres`): `meada postgres migrate` and the `postgres:`
//! line of `meada status --workspace`, as child processes. The URL refusals need no service;
//! the rest run against the local Postgres `MEADA_TEST_POSTGRES_URL` names, each test in a schema
//! of its own, and print why and skip without it (fail with `MEADA_TEST_SERVICES_REQUIRED=1`).
#![cfg(feature = "postgres")]

use std::fs;
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::str::FromStr;
use std::time::{Duration, Instant};

use crate::manifest::{MANIFEST, chain_manifest, run};

/// The local Postgres `MEADA_TEST_POSTGRES_URL` names, or `None` with the reason printed;
/// `MEADA_TEST_SERVICES_REQUIRED=1` turns the missing variable into a failure.
pub(crate) fn service() -> Option<String> {
    let url = std::env::var("MEADA_TEST_POSTGRES_URL").ok();
    if url.is_none() {
        let required = std::env::var("MEADA_TEST_SERVICES_REQUIRED").is_ok_and(|v| v == "1");
        assert!(!required, "MEADA_TEST_POSTGRES_URL is not set");
        eprintln!("skipped: MEADA_TEST_POSTGRES_URL is not set");
    }
    url
}

/// A schema of this test's own, dropped first in case a previous run left it.
pub(crate) fn schema(case: &str) -> String {
    format!("meada_t{}_{case}", std::process::id())
}

/// `<case>/ops/meada.workspace.toml` with `index_dir` and a `[postgres]` section of `lines`.
pub(crate) fn manifest(case: &str, lines: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(case);
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join("ops")).unwrap();
    let toml = format!("index_dir = \"../indexes\"\n\n[postgres]\n{lines}");
    fs::write(dir.join(MANIFEST), toml).unwrap();
    dir
}

/// `meada <args>` in `dir` under `lang` with `env` set: stdout, stderr and the exit code.
pub(crate) fn meada(
    dir: &Path,
    lang: &str,
    env: &[(&str, &str)],
    args: &[&str],
) -> (String, String, i32) {
    let mut command = Command::new(env!("CARGO_BIN_EXE_meada"));
    command.current_dir(dir).args(args).env("LANG", lang);
    command.env_remove("MEADA_POSTGRES_URL");
    command.env_remove("MEADA_POSTGRES_VERSION_MAX");
    let out = command
        .envs(env.iter().copied())
        .output()
        .expect("meada runs");
    let text = |bytes: &[u8]| String::from_utf8_lossy(bytes).into_owned();
    let code = out.status.code().expect("an exit code");
    (text(&out.stdout), text(&out.stderr), code)
}

pub(crate) const MIGRATE: [&str; 4] = ["postgres", "migrate", "--workspace", MANIFEST];
pub(crate) const STATUS: [&str; 3] = ["status", "--workspace", MANIFEST];
const EN: &str = "en_US.UTF-8";

/// `meada postgres migrate` with the URL in `MEADA_POSTGRES_URL` and the version capped at
/// `cap` when given.
fn migrate(dir: &Path, url: &str, cap: Option<&str>) -> (String, String, i32) {
    let mut env = vec![("MEADA_POSTGRES_URL", url)];
    env.extend(cap.map(|cap| ("MEADA_POSTGRES_VERSION_MAX", cap)));
    meada(dir, EN, &env, &MIGRATE)
}

/// The `postgres:` line of `meada status --workspace`, as `migrate`.
fn status_line(dir: &Path, url: &str, cap: Option<&str>) -> String {
    let mut env = vec![("MEADA_POSTGRES_URL", url)];
    env.extend(cap.map(|cap| ("MEADA_POSTGRES_VERSION_MAX", cap)));
    let (status, _, _) = meada(dir, EN, &env, &STATUS);
    let line = status.lines().find(|l| l.starts_with("postgres: "));
    line.unwrap_or_else(|| panic!("no postgres line: {status}"))
        .to_string()
}

/// A plain connection to the test's Postgres, for what the tests set up and look at.
pub(crate) struct Admin {
    client: tokio_postgres::Client,
    runtime: tokio::runtime::Runtime,
}

impl Admin {
    pub(crate) fn new(url: &str) -> Self {
        let runtime = (tokio::runtime::Builder::new_current_thread())
            .enable_all()
            .build()
            .unwrap();
        let config = tokio_postgres::Config::from_str(url).unwrap();
        let (host, port) = match (&config.get_hosts()[0], config.get_ports().first()) {
            (tokio_postgres::config::Host::Tcp(host), port) => {
                (host.clone(), *port.unwrap_or(&5432))
            }
            _ => panic!("MEADA_TEST_POSTGRES_URL names no TCP host"),
        };
        let client = runtime.block_on(async {
            let tcp = tokio::net::TcpStream::connect((host.as_str(), port))
                .await
                .unwrap();
            let (client, connection) = (config.connect_raw(tcp, tokio_postgres::NoTls))
                .await
                .unwrap();
            tokio::spawn(connection);
            client
        });
        Self { client, runtime }
    }

    pub(crate) fn run(&self, sql: &str) {
        let done = self.runtime.block_on(self.client.batch_execute(sql));
        done.unwrap_or_else(|e| panic!("{sql}: {e}"));
    }

    /// The first column of every row of `sql`, as text.
    pub(crate) fn texts(&self, sql: &str) -> Vec<String> {
        let rows = self
            .runtime
            .block_on(self.client.simple_query(sql))
            .unwrap();
        (rows.iter())
            .filter_map(|row| match row {
                tokio_postgres::SimpleQueryMessage::Row(row) => Some(row.get(0)?.to_string()),
                _ => None,
            })
            .collect()
    }

    /// What the schema holds: each table with its columns, and the version row with its `xmin`,
    /// which any write to it changes.
    fn snapshot(&self, schema: &str) -> Vec<String> {
        let mut held = self.texts(&format!(
            "SELECT table_name || '.' || column_name || ' ' || data_type FROM \
             information_schema.columns WHERE table_schema = '{schema}' ORDER BY 1"
        ));
        held.extend(self.texts(&format!(
            "SELECT version || ' ' || xmin FROM \"{schema}\".schema_version"
        )));
        held
    }
}

/// The key of `schema`'s advisory lock, as the module documents it.
fn lock_key(schema: &str) -> i64 {
    let hash = blake3::hash(format!("meada postgres migrate {schema}").as_bytes());
    i64::from_le_bytes(hash.as_bytes()[..8].try_into().unwrap())
}

/// Each URL refusal names `url_env` or `url_file` and never the URL, its password or its host,
/// in `migrate`'s error and in `status`'s line; no connection is attempted.
#[test]
fn every_url_refusal_names_its_key_and_no_part_of_the_url() {
    let (password, host) = ("hunter2-327", "db-327.example.internal");
    let at = |rest: &str| format!("postgres://meada:{password}@{host}{rest}");
    let urls = [
        format!("mysql://meada:{password}@{host}/meada"),
        format!("postgres://meada:{password}@%2Fvar%2Frun%2F{host}/meada"),
        at("/meada?application_name=x"),
        at("/meada?sslmode=disable"),
        at(":0/meada"),
    ];
    let dir = manifest("postgres_url_refusals", "schema = \"meada\"\n");
    for url in &urls {
        let (stdout, stderr, code) = migrate(&dir, url, None);
        assert!(code == 1 && stdout.is_empty(), "{url}: {stderr}");
        assert!(
            stderr.starts_with("meada: `postgres.url_env`: "),
            "{stderr}"
        );
        let line = status_line(&dir, url, None);
        assert!(
            line.starts_with("postgres: unreachable (`postgres.url_env`: "),
            "{line}"
        );
        for text in [&stderr, &line] {
            for part in [url.as_str(), password, host, "sslmode", "application_name"] {
                assert!(!text.contains(part), "{part} in {text}");
            }
        }
    }
    // An unset variable names it; an unreadable file names the key and the file.
    let (_, stderr, code) = meada(&dir, EN, &[], &MIGRATE);
    assert_eq!(code, 1);
    assert_eq!(
        stderr,
        "meada: `postgres.url_env`: the variable MEADA_POSTGRES_URL is not set\n"
    );
    let dir = manifest(
        "postgres_url_file_refusals",
        "url_file = \"../pg-url\"\nschema = \"meada\"\n",
    );
    let (_, stderr, code) = meada(&dir, EN, &[], &MIGRATE);
    assert!(code == 1, "{stderr}");
    assert!(
        stderr.starts_with("meada: `postgres.url_file`: "),
        "{stderr}"
    );
    assert!(stderr.contains("pg-url"), "{stderr}");
    // A file holding a refused URL names the key alone, with its trailing newline dropped.
    fs::write(
        dir.join("pg-url"),
        format!("{}\n", at("/m?sslmode=disable")),
    )
    .unwrap();
    let (_, stderr, code) = meada(&dir, EN, &[], &MIGRATE);
    assert!(code == 1, "{stderr}");
    assert_eq!(
        stderr,
        "meada: `postgres.url_file`: holds a URL with a query: no parameter is accepted, TLS is \
         decided by the host\n"
    );
}

/// `migrate` without `[postgres]` names the manifest; `meada index` with it opens no connection.
#[test]
fn migrate_needs_the_section_and_index_never_connects() {
    let dir = manifest("postgres_no_section", "");
    fs::write(dir.join(MANIFEST), "index_dir = \"../indexes\"\n").unwrap();
    let (_, stderr, code) = meada(&dir, EN, &[], &MIGRATE);
    assert_eq!(code, 1);
    assert_eq!(
        stderr,
        format!("meada: {MANIFEST}: no [postgres] section: meada postgres migrate needs one\n")
    );
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("postgres://meada@{}/meada", listener.local_addr().unwrap());
    let dir = chain_manifest("postgres_index_offline");
    let ops = dir.join(MANIFEST);
    let toml = fs::read_to_string(&ops).unwrap();
    let (head, entries) = toml.split_once("\n[[repository]]").unwrap();
    let section = "\n[postgres]\nschema = \"meada\"\n";
    fs::write(&ops, format!("{head}{section}\n[[repository]]{entries}")).unwrap();
    let (_, stderr, code) = meada(
        &dir,
        EN,
        &[("MEADA_POSTGRES_URL", &url)],
        &["index", "--workspace", MANIFEST],
    );
    assert_eq!(code, 0, "{stderr}");
    listener.set_nonblocking(true).unwrap();
    assert!(
        listener.accept().is_err(),
        "meada index connected to Postgres"
    );
}

/// `migrate` on an empty schema creates it at the binary's version and prints it; a second run
/// prints it again and changes nothing.
#[test]
fn migrate_creates_the_schema_once_and_a_second_run_changes_nothing() {
    let Some(url) = service() else { return };
    let schema = schema("migrate");
    let admin = Admin::new(&url);
    admin.run(&format!("DROP SCHEMA IF EXISTS \"{schema}\" CASCADE"));
    let dir = manifest("postgres_migrate", &format!("schema = \"{schema}\"\n"));
    // Every other test's schema is `meada_t*`: nothing else may change.
    let outside = || {
        admin.texts(
            "SELECT table_schema || '.' || table_name FROM information_schema.tables WHERE \
             table_schema NOT LIKE 'meada\\_t%' ORDER BY 1",
        )
    };
    let before = outside();
    let (stdout, stderr, code) = migrate(&dir, &url, None);
    assert_eq!((stdout.as_str(), stderr.as_str(), code), ("1\n", "", 0));
    assert_eq!(outside(), before);
    let first = admin.snapshot(&schema);
    assert_eq!(first[0], "schema_version.version integer");
    assert!(first[1].starts_with("1 "), "{first:?}");
    let (stdout, stderr, code) = migrate(&dir, &url, None);
    assert_eq!((stdout.as_str(), stderr.as_str(), code), ("1\n", "", 0));
    assert_eq!(admin.snapshot(&schema), first);
    // A schema the operator created beforehand, empty, is kept and migrated.
    admin.run(&format!(
        "DROP SCHEMA \"{schema}\" CASCADE; CREATE SCHEMA \"{schema}\""
    ));
    let (stdout, _, code) = migrate(&dir, &url, None);
    assert_eq!((stdout.as_str(), code), ("1\n", 0));
    admin.run(&format!("DROP SCHEMA \"{schema}\" CASCADE"));
}

/// Two runs serialize on the schema's advisory lock: one waits while the lock is held, and two
/// started together both succeed at the binary's version.
#[test]
fn two_migrations_serialize_on_the_advisory_lock() {
    let Some(url) = service() else { return };
    let schema = schema("lock");
    let admin = Admin::new(&url);
    admin.run(&format!("DROP SCHEMA IF EXISTS \"{schema}\" CASCADE"));
    let dir = manifest("postgres_lock", &format!("schema = \"{schema}\"\n"));
    let spawn = || {
        Command::new(env!("CARGO_BIN_EXE_meada"))
            .current_dir(&dir)
            .args(MIGRATE)
            .env("LANG", EN)
            .env("MEADA_POSTGRES_URL", &url)
            .env_remove("MEADA_POSTGRES_VERSION_MAX")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap()
    };
    let key = lock_key(&schema);
    admin.run(&format!("SELECT pg_advisory_lock({key})"));
    let mut waiting = spawn();
    let start = Instant::now();
    while start.elapsed() < Duration::from_millis(1500) {
        assert!(
            waiting.try_wait().unwrap().is_none(),
            "migrate ran without the lock"
        );
        std::thread::sleep(Duration::from_millis(100));
    }
    let created = admin.texts(&format!(
        "SELECT nspname FROM pg_namespace WHERE nspname = '{schema}'"
    ));
    assert!(created.is_empty(), "migrate wrote before taking the lock");
    admin.run(&format!("SELECT pg_advisory_unlock({key})"));
    let out = waiting.wait_with_output().unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(out.stdout, b"1\n");

    admin.run(&format!("DROP SCHEMA \"{schema}\" CASCADE"));
    let (one, two) = (spawn(), spawn());
    for out in [
        one.wait_with_output().unwrap(),
        two.wait_with_output().unwrap(),
    ] {
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert_eq!(out.stdout, b"1\n");
    }
    admin.run(&format!("DROP SCHEMA \"{schema}\" CASCADE"));
}

/// With the debug knob: a lower stored version (an absent schema) makes `status` print the
/// unreachable form naming `meada postgres migrate`, a higher one the form naming both versions,
/// and `migrate` by a capped binary refuses to lower it, naming both.
#[test]
fn a_version_this_binary_does_not_know_is_refused() {
    let Some(url) = service() else { return };
    let schema = schema("version");
    let admin = Admin::new(&url);
    admin.run(&format!("DROP SCHEMA IF EXISTS \"{schema}\" CASCADE"));
    let dir = manifest("postgres_version", &format!("schema = \"{schema}\"\n"));
    // Capped at 0, migrate leaves the schema absent: version 0, lower than the binary's.
    let (stdout, _, code) = migrate(&dir, &url, Some("0"));
    assert_eq!((stdout.as_str(), code), ("0\n", 0));
    assert_eq!(
        status_line(&dir, &url, None),
        format!(
            "postgres: unreachable (the schema {schema} is at version 0, below this Meada's 1: \
             run meada postgres migrate)"
        )
    );
    assert_eq!(migrate(&dir, &url, None).0, "1\n");
    let higher =
        format!("the schema {schema} is at version 1, above this Meada's 0: upgrade Meada");
    assert_eq!(
        status_line(&dir, &url, Some("0")),
        format!("postgres: unreachable ({higher})")
    );
    let before = admin.snapshot(&schema);
    let (stdout, stderr, code) = migrate(&dir, &url, Some("0"));
    assert_eq!((stdout.as_str(), code), ("", 1));
    assert_eq!(stderr, format!("meada: {higher}\n"));
    assert_eq!(admin.snapshot(&schema), before);
    admin.run(&format!("DROP SCHEMA \"{schema}\" CASCADE"));
}

/// `status` prints the reachable form against the local Postgres and the unreachable one against
/// a closed port, with the exit code it has without `[postgres]`, indexed or not.
#[test]
fn status_names_whether_postgres_is_reachable_and_its_exit_code_ignores_it() {
    let dir = chain_manifest("postgres_status");
    let ops = dir.join(MANIFEST);
    let plain = fs::read_to_string(&ops).unwrap();
    let with = |lines: &str| {
        let (head, entries) = plain.split_once("\n[[repository]]").unwrap();
        fs::write(
            &ops,
            format!("{head}\n[postgres]\n{lines}\n[[repository]]{entries}"),
        )
        .unwrap();
    };
    let closed = "postgres://meada@127.0.0.1:1/meada";
    for indexed in [false, true] {
        if indexed {
            assert!(run(&dir, "index").2);
        }
        fs::write(&ops, &plain).unwrap();
        let (_, _, without) = meada(&dir, EN, &[], &STATUS);
        with("schema = \"meada\"\n");
        let (status, stderr, code) = meada(&dir, EN, &[("MEADA_POSTGRES_URL", closed)], &STATUS);
        assert_eq!(code, without, "{stderr}");
        assert!(stderr.is_empty(), "{stderr}");
        assert!(status.contains("\npostgres: unreachable ("), "{status}");
        assert_eq!(code == 0, indexed);
    }
    let Some(url) = service() else { return };
    let schema = schema("status");
    let admin = Admin::new(&url);
    admin.run(&format!("DROP SCHEMA IF EXISTS \"{schema}\" CASCADE"));
    with(&format!("schema = \"{schema}\"\n"));
    assert_eq!(migrate(&dir, &url, None).0, "1\n");
    let env = [("MEADA_POSTGRES_URL", url.as_str())];
    let (status, stderr, code) = meada(&dir, EN, &env, &STATUS);
    assert!(code == 0 && stderr.is_empty(), "{stderr}");
    let line = format!("\npostgres: reachable, schema {schema} version 1\n");
    assert!(status.ends_with(&line), "{status}");
    // A `ca_file` on loopback is never ignored: the plain test server refuses the TLS it asks.
    let roots = "/etc/ssl/certs/ca-certificates.crt";
    if Path::new(roots).is_file() {
        with(&format!("schema = \"{schema}\"\nca_file = \"{roots}\"\n"));
        let (status, _, code) = meada(&dir, EN, &env, &STATUS);
        assert_eq!(code, 0);
        let refused = "postgres: unreachable (error performing TLS handshake: server does not \
                       support TLS)";
        assert!(status.contains(refused), "{status}");
    }
    admin.run(&format!("DROP SCHEMA \"{schema}\" CASCADE"));
}
