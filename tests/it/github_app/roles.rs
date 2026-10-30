//! The role check (#331, ADR 0016): `meada github-app roles` against the fake GitHub API of
//! `super` (its collaborators listing), the local Valkey `MEADA_TEST_VALKEY_URL` names and the
//! local Postgres `MEADA_TEST_POSTGRES_URL` names, each test in a schema and a key prefix of its
//! own; and the readers of the pulled version honoring the exclusions file it writes. No test
//! reads the module's tables: the set is seen through the command's line, its fence refusal and
//! the readers. Without a service the tests that need it print why and skip; with
//! `MEADA_TEST_SERVICES_REQUIRED=1` they fail.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use super::{Api, fixture, prefix};
use crate::manifest::{CHAIN, MANIFEST, chain_manifest};

/// The bootstrap manifest of a managed workspace, beside the chain's own.
const BOOTSTRAP: &str = "ops/bootstrap.toml";
/// Where its pulled version lives.
const PULLED: &str = "store-indexes/workspace";
/// The git host of the chain's identities, and of `[oidc.github]`.
const HOST: &str = "example.com";

const WEB: &str = "example.com/chain/web";
const GATEWAY: &str = "example.com/chain/gateway";
const CATALOG: &str = "example.com/chain/catalog";
const WORKER: &str = "example.com/chain/worker";

/// A case of the role check: the chain's mirrors in `dir`, the fake API with an installation
/// per App of `apps` on the owner `chain` (installation `10 * app + 1`), and the Postgres URL
/// and schema of its own.
struct Case {
    dir: PathBuf,
    api: Api,
    case: &'static str,
    valkey: String,
    url: String,
    schema: String,
    apps: Vec<u64>,
}

impl Case {
    fn new(case: &'static str, apps: &[u64]) -> Option<Self> {
        let valkey = super::service()?;
        let url = crate::postgres::service()?;
        let dir = chain_manifest(case);
        let schema = crate::postgres::schema(case);
        crate::postgres::Admin::new(&url).run(&format!("DROP SCHEMA IF EXISTS {schema} CASCADE"));
        fs::write(dir.join("ops/pg-url"), &url).unwrap();
        fs::write(dir.join("ops/s3-secret"), "x\n").unwrap();
        let api = Api::new();
        for app in apps {
            api.install(*app, 10 * app + 1, "chain");
        }
        Some(Self {
            dir,
            api,
            case,
            valkey,
            url,
            schema,
            apps: apps.to_vec(),
        })
    }

    /// `[postgres]`, `[oidc]` with `[oidc.github]` (`roles` as given, else the default) and a
    /// `[[github_app]]` block per App, `with_valkey` adding the case's `[valkey]`.
    fn sections(&self, roles: Option<&str>, with_valkey: bool) -> String {
        let mut toml = String::new();
        if with_valkey {
            toml += &format!(
                "\n[valkey]\nurl = \"{}\"\nkey_prefix = \"{}\"\n",
                self.valkey,
                prefix(self.case)
            );
        }
        let roles = roles.map_or(String::new(), |r| format!("roles = {r}\n"));
        toml += &format!(
            "\n[postgres]\nurl_file = \"pg-url\"\nschema = \"{}\"\n\n[oidc]\nissuer = \
             \"https://id.example\"\npublic_url = \"https://meada.example\"\n\n[oidc.github]\n\
             host = \"{HOST}\"\nlogin_claim = \"github_login\"\n{roles}max_age_seconds = 5400\n",
            self.schema
        );
        for app in &self.apps {
            let key = fixture(&format!("app{app}.pem"));
            toml += &format!(
                "\n[[github_app]]\nhost = \"{HOST}\"\napi = \"{}\"\napp_id = {app}\n\
                 private_key_file = \"{}\"\n",
                self.api.base,
                key.display()
            );
        }
        toml
    }

    /// `ops/bootstrap.toml`: `[workspace] source = "store"` over `../store-indexes`, a
    /// `[snapshot]` nothing connects to, `[generations]` and, with `fenced`, `[refresh_lock]`.
    fn bootstrap(&self, roles: Option<&str>, fenced: bool) {
        let lock = if fenced {
            "\n[refresh_lock]\nttl_seconds = 10\n"
        } else {
            ""
        };
        let toml = format!(
            "index_dir = \"../store-indexes\"\n{}\n[generations]\nkeep = 3\n{lock}{}\n\
             [workspace]\nsource = \"store\"\n",
            crate::snapshot::section("http://127.0.0.1:9", "unused"),
            self.sections(roles, true)
        );
        fs::write(self.dir.join(BOOTSTRAP), toml).unwrap();
        let _ = fs::remove_dir_all(self.dir.join("store-indexes"));
        let migrated = meada(
            &self.dir,
            &["postgres", "migrate", "--workspace", BOOTSTRAP],
            &[],
        );
        assert_eq!(migrated.2, Some(0), "{}", migrated.1);
    }

    /// Lists `collaborators` (login, role) for `<owner>/<repo>`.
    fn list(&self, repo: &str, collaborators: &[(&str, &str)]) {
        let rows = (collaborators.iter())
            .map(|(l, r)| (l.to_string(), r.to_string()))
            .collect();
        self.api
            .with(|s| s.collaborators.insert(repo.to_string(), rows));
    }

    /// `meada github-app roles --workspace <manifest>` with `fence` as `MEADA_REFRESH_FENCE`,
    /// its stderr checked for leaks.
    fn roles(&self, manifest: &str, fence: Option<u64>) -> (String, String, Option<i32>) {
        let fence = fence.map(|f| f.to_string());
        let env: Vec<(&str, &str)> = fence
            .iter()
            .map(|f| ("MEADA_REFRESH_FENCE", f.as_str()))
            .collect();
        let out = meada(
            &self.dir,
            &["github-app", "roles", "--workspace", manifest],
            &env,
        );
        self.clean(&out.1);
        out
    }

    /// `roles` that succeeds: the set's id from its line, which must read `counts`.
    fn checked(&self, manifest: &str, fence: Option<u64>, counts: &str) -> (String, String) {
        let (stdout, stderr, code) = self.roles(manifest, fence);
        assert_eq!(code, Some(0), "{stderr}");
        let (id, rest) = (stdout.strip_prefix("permissions set "))
            .and_then(|s| s.split_once(": "))
            .unwrap_or_else(|| panic!("{stdout}"));
        assert!(
            id.len() == 32 && id.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')),
            "{id}"
        );
        assert_eq!(rest, format!("{counts}\n"));
        (id.to_string(), stderr)
    }

    /// Asserts `stderr` holds no token, JWT or key (`Api::clean`), no login GitHub lists or an
    /// entry is attributed to, and neither the Postgres URL nor its password.
    fn clean(&self, stderr: &str) {
        self.api.clean(stderr);
        let lower = stderr.to_lowercase();
        self.api.with(|s| {
            for (login, _) in s.collaborators.values().flatten() {
                assert!(!lower.contains(&login.to_lowercase()), "{login}: {stderr}");
            }
        });
        for login in ["ana-souza", "bia_acme", "caio"] {
            assert!(!lower.contains(login), "{login}: {stderr}");
        }
        assert!(!stderr.contains(&self.url), "{stderr}");
        let secret = self
            .url
            .split_once("://")
            .and_then(|(_, r)| r.split_once('@'));
        if let Some((userinfo, _)) = secret {
            assert!(!stderr.contains(userinfo), "{stderr}");
        }
    }

    /// The exclusions file of the pulled version, when there is one.
    fn excluded(&self) -> Option<String> {
        fs::read_to_string(self.dir.join(PULLED).join("excluded")).ok()
    }

    /// Every listing request the fake saw, as installation and path.
    fn listings(&self) -> Vec<(u64, String)> {
        self.api.with(|s| s.listings.clone())
    }
}

/// `meada <args>` in `dir` with `env`, in English, no fence or Postgres URL inherited.
fn meada(dir: &Path, args: &[&str], env: &[(&str, &str)]) -> (String, String, Option<i32>) {
    let out = command(dir, args, env).output().expect("meada runs");
    let text = |bytes: &[u8]| String::from_utf8_lossy(bytes).into_owned();
    (text(&out.stdout), text(&out.stderr), out.status.code())
}

fn command(dir: &Path, args: &[&str], env: &[(&str, &str)]) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_meada"));
    command
        .current_dir(dir)
        .args(args)
        .env("LANG", "en_US.UTF-8")
        .env("MEADA", env!("CARGO_BIN_EXE_meada"))
        .env("MEADA_TEST_S3_KEY", crate::snapshot::KEY_ID)
        .env_remove("MEADA_TEST_S3_TOKEN")
        .env_remove("MEADA_REFRESH_LOCK")
        .env_remove("MEADA_REFRESH_FENCE")
        .env_remove("MEADA_POSTGRES_URL")
        .envs(env.iter().copied());
    command
}

/// `meada <args> --workspace ops/bootstrap.toml` in `dir`.
fn on_bootstrap(dir: &Path, args: &[&str]) -> (String, String, Option<i32>) {
    meada(dir, &[args, &["--workspace", BOOTSTRAP]].concat(), &[])
}

/// `entries` (identity, path, `added_by`) as the workspace version pulled into `<dir>/PULLED`
/// at fence 1, as `meada workspace pull` leaves it: its id. An exclusions file stays.
fn pull_version(dir: &Path, entries: &[(&str, &str, Option<&str>)]) -> String {
    let blocks = serde_json::json!({});
    let hashed: Vec<meada_core::ids::VersionEntry> = (entries.iter())
        .map(|(repo, path, login)| (*repo, *path, &blocks, None, *login))
        .collect();
    let id = meada_core::ids::workspace_version_id(&hashed);
    let repositories: Vec<serde_json::Value> = (entries.iter())
        .map(|(repo, path, login)| {
            let mut entry = serde_json::json!({"repo": repo, "path": path});
            if let Some(login) = login {
                entry["added_by"] = (*login).into();
            }
            entry
        })
        .collect();
    let document = serde_json::json!({
        "id": id, "author": "alice", "time": "2026-10-29T00:00:00Z", "message": "chain",
        "repositories": repositories,
    });
    let dir = dir.join(PULLED);
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join(format!("{id}.json")), document.to_string()).unwrap();
    fs::write(dir.join("current"), format!("{id} 1\n")).unwrap();
    id
}

/// The chain's entries, `logins[n]` the `added_by` of the n-th.
fn chain(
    logins: [Option<&'static str>; 4],
) -> Vec<(&'static str, &'static str, Option<&'static str>)> {
    let paths = [
        "../chain-web",
        "../chain-gateway",
        "../chain-catalog",
        "../chain-worker",
    ];
    (CHAIN.iter().zip(paths).zip(logins))
        .map(|(((repo, _), path), login)| (*repo, path, login))
        .collect()
}

/// `n` collaborators `u0000`, `u0001`... with `role`.
fn many(n: usize, role: &'static str) -> Vec<(String, &'static str)> {
    (0..n).map(|i| (format!("u{i:04}"), role)).collect()
}

/// `roles` lists every page of a 250-collaborator repository, each request with
/// `affiliation=all` and `per_page=100`, and writes one set with every row whatever its role and
/// a login checked ignoring case; a 404 and an owner no installation covers each give a warning
/// naming the identity and no rows, and the run succeeds. With nothing to exclude the exclusions
/// file holds the version id alone. A second run on the same version replaces the set (another
/// id) and writes the same file.
#[test]
fn the_role_check_lists_every_page_and_replaces_the_set() {
    let Some(case) = Case::new("roles_pages", &[1]) else {
        return;
    };
    case.bootstrap(None, false);
    let mut entries = chain([Some("ana-souza"), None, None, None]);
    entries.push(("example.com/elsewhere/tool", "../chain-web", None));
    let version = pull_version(&case.dir, &entries);
    let mut web = many(249, "read");
    web.insert(120, ("Ana-Souza".into(), "write"));
    let web: Vec<(&str, &str)> = web.iter().map(|(l, r)| (l.as_str(), *r)).collect();
    case.list("chain/web", &web);
    case.list("chain/gateway", &[("u9000", "admin")]);
    case.list("chain/catalog", &[("u9001", "custom-role")]);

    let (first, stderr) = case.checked(BOOTSTRAP, None, "3 repositories, 252 rows, 0 exclusions");
    assert_eq!(
        stderr,
        format!(
            "meada: {WORKER}: GitHub answers 404 for its collaborators; it gets no rows\n\
             meada: example.com/elsewhere/tool: no GitHub App installation covers its owner; it \
             gets no rows\n"
        )
    );
    let listings = case.listings();
    assert!(
        listings
            .iter()
            .all(|(_, path)| path.contains("affiliation=all") && path.contains("per_page=100")),
        "{listings:?}"
    );
    let pages: Vec<&str> = (listings.iter())
        .filter(|(_, path)| path.starts_with("/repos/chain/web/"))
        .map(|(_, path)| path.rsplit('&').next().unwrap())
        .collect();
    assert_eq!(pages, ["page=1", "page=2", "page=3"]);
    assert_eq!(case.excluded(), Some(format!("{version}\n")));

    let (second, _) = case.checked(BOOTSTRAP, None, "3 repositories, 252 rows, 0 exclusions");
    assert_ne!(first, second);
    assert_eq!(case.excluded(), Some(format!("{version}\n")));
}

/// With `source = "file"` the role check lists the file's entries, writes no exclusions file
/// and reads none. Under `[refresh_lock]` it needs `MEADA_REFRESH_FENCE`, a set is replaced at an
/// equal or higher fence and refused at a lower one, naming the higher. A 403 on one
/// installation marks it and passes to the next covering the owner; with every one throttled,
/// or on a 5xx, the run exits 1 with one line naming the identity and the status, and the set
/// stays: a lower fence is still refused naming its fence.
#[cfg(feature = "refresh-lock")]
#[test]
fn a_set_is_never_replaced_by_a_lower_fence_or_a_failed_listing() {
    let Some(case) = Case::new("roles_fence", &[1, 2]) else {
        return;
    };
    let dir = &case.dir;
    let sections = format!(
        "{}\n[generations]\nkeep = 3\n\n[refresh_lock]\nttl_seconds = 10\n{}",
        crate::snapshot::section("http://127.0.0.1:9", "unused"),
        case.sections(None, true)
    );
    crate::snapshot::append_section(dir, &sections);
    let migrated = meada(dir, &["postgres", "migrate", "--workspace", MANIFEST], &[]);
    assert_eq!(migrated.2, Some(0), "{}", migrated.1);
    case.list(
        "chain/web",
        &[("u0001", "write"), ("u0002", "read"), ("u0003", "admin")],
    );
    case.list("chain/gateway", &[("u0004", "write")]);
    case.list("chain/catalog", &[("u0005", "maintain")]);
    let rows = "3 repositories, 5 rows, 0 exclusions";
    let not_found =
        format!("meada: {WORKER}: GitHub answers 404 for its collaborators; it gets no rows\n");

    let unfenced = case.roles(MANIFEST, None);
    let env = "meada: [refresh_lock] needs MEADA_REFRESH_FENCE: run meada github-app roles under \
               meada snapshot lock\n";
    assert_eq!(unfenced, (String::new(), env.to_string(), Some(1)));
    let (first, stderr) = case.checked(MANIFEST, Some(5), rows);
    assert_eq!(stderr, not_found);
    assert!(!dir.join("indexes/workspace").exists());
    let (again, _) = case.checked(MANIFEST, Some(5), rows);
    assert_ne!(first, again);
    let below = |held: u64| {
        let line = format!(
            "meada: the newest permissions set has fence {held}, above this run's 4: a newer \
             refresh wrote it; nothing was written\n"
        );
        assert_eq!(
            case.roles(MANIFEST, Some(4)),
            (String::new(), line, Some(1))
        );
    };
    below(5);
    // In Portuguese too, like every line of the role check.
    let pt = [("LANG", "pt_BR.UTF-8"), ("MEADA_REFRESH_FENCE", "4")];
    let (_, stderr, _) = meada(dir, &["github-app", "roles", "--workspace", MANIFEST], &pt);
    let line = "meada: o conjunto de permissões mais novo tem fence 5, acima do 4 desta execução: \
                um refresh mais novo o gravou; nada foi gravado\n";
    assert_eq!(stderr, line);

    // A 403 without `retry-after` marks installation 11 for a minute; the listing passes to 21.
    case.api
        .with(|s| s.listing_throttled.insert(11, (403, None)));
    let by = |id: u64| case.listings().iter().filter(|(i, _)| *i == id).count();
    let tried = by(11);
    let (_, stderr) = case.checked(MANIFEST, Some(6), rows);
    assert!(
        stderr.contains("meada: installation 11: throttled for 60 s\n"),
        "{stderr}"
    );
    assert_eq!(by(11), tried + 1);
    // A 429 with `retry-after` on 21, 11 still marked: every one is throttled.
    case.api
        .with(|s| s.listing_throttled.insert(21, (429, Some(30))));
    let (stdout, stderr, code) = case.roles(MANIFEST, Some(7));
    assert_eq!((stdout.as_str(), code), ("", Some(1)));
    assert_eq!(
        stderr,
        format!(
            "meada: installation 21: throttled for 30 s\nmeada: {WEB}: the role check failed: \
             every installation covering its owner is throttled (http status 429); nothing was \
             written\n"
        )
    );
    below(6);
    // Both marked: the run fails before any listing.
    let before = case.listings().len();
    let (_, stderr, code) = case.roles(MANIFEST, Some(7));
    assert_eq!(code, Some(1));
    assert_eq!(
        stderr,
        format!(
            "meada: {WEB}: the role check failed: every installation covering its owner is \
             throttled; nothing was written\n"
        )
    );
    assert_eq!(case.listings().len(), before);

    // The marks gone, a 5xx fails the run alike.
    let mut valkey = super::admin(&case.valkey);
    for id in [11, 21] {
        let key = format!(
            "{}:github-app:{HOST}:installation:{id}:throttled",
            prefix(case.case)
        );
        let _: () = redis::cmd("DEL").arg(key).query(&mut valkey).unwrap();
    }
    case.api.with(|s| {
        s.listing_throttled.clear();
        s.listing_status = Some(503);
    });
    let (stdout, stderr, code) = case.roles(MANIFEST, Some(8));
    assert_eq!((stdout.as_str(), code), ("", Some(1)));
    assert!(
        stderr.starts_with(&format!(
            "meada: {WEB}: the role check failed: {}/repos/chain/web/collaborators?",
            case.api.base
        )) && stderr.ends_with(": http status 503; nothing was written\n"),
        "{stderr}"
    );
    below(6);
}

/// An addition whose `added_by` GitHub lists with no role, or only with one outside `roles`,
/// is excluded: named with the reason on stderr (never the login), counted in the set, in the
/// exclusions file, absent from `show --pulled`, not indexed and absent from `status` and
/// `export`. An entry without `added_by` is never excluded. Once GitHub lists an accepted role
/// for each, a custom role `roles` names included, the next check includes them.
#[test]
fn an_addition_without_an_accepted_role_is_left_out_by_every_reader() {
    let Some(case) = Case::new("roles_excluded", &[1]) else {
        return;
    };
    let dir = &case.dir;
    case.bootstrap(
        Some("[\"read\", \"triage\", \"write\", \"maintain\", \"admin\", \"release-manager\"]"),
        false,
    );
    let version = pull_version(
        dir,
        &chain([Some("ana-souza"), Some("bia_acme"), Some("caio"), None]),
    );
    case.list("chain/web", &[("ana-souza", "write"), ("u0001", "read")]);
    case.list("chain/gateway", &[("Bia_Acme", "custom-reviewer")]);
    case.list("chain/catalog", &[("u0002", "admin")]);

    let (_, stderr) = case.checked(BOOTSTRAP, None, "3 repositories, 4 rows, 2 exclusions");
    assert_eq!(
        stderr,
        format!(
            "meada: {WORKER}: GitHub answers 404 for its collaborators; it gets no rows\n\
             meada: {GATEWAY}: excluded: its added_by holds no role in [oidc.github] roles\n\
             meada: {CATALOG}: excluded: GitHub lists no role for its added_by\n"
        )
    );
    assert_eq!(
        case.excluded(),
        Some(format!("{version}\n{CATALOG}\n{GATEWAY}\n"))
    );
    let (shown, stderr, code) = on_bootstrap(dir, &["workspace", "show", "--pulled"]);
    assert_eq!((stderr.as_str(), code), ("", Some(0)));
    assert_eq!(
        shown,
        format!(
            "[[repository]]\npath = \"../chain-web\"\nadded_by = \"ana-souza\"\nrepo = \"{WEB}\"\n\n\
             [[repository]]\npath = \"../chain-worker\"\nrepo = \"{WORKER}\"\n"
        )
    );
    let (_, stderr, code) = on_bootstrap(dir, &["index"]);
    assert_eq!((stderr.as_str(), code), ("", Some(0)));
    let indexed = |repo: &str| dir.join("store-indexes").join(repo).exists();
    assert!(indexed(WEB) && indexed(WORKER) && !indexed(GATEWAY) && !indexed(CATALOG));
    let (status, _, code) = on_bootstrap(dir, &["status"]);
    assert_eq!(code, Some(0));
    let listed = |repo: &str| status.lines().any(|l| l.starts_with(&format!("{repo} (")));
    assert!(listed(WEB) && listed(WORKER), "{status}");
    assert!(!listed(GATEWAY) && !listed(CATALOG), "{status}");
    // Absent as an entry the workspace does not list: a peer that names it misses it.
    let missing = format!("missing peer: {GATEWAY}, named by {WEB}\n");
    assert!(status.contains(&missing), "{status}");
    let (_, stderr, code) = on_bootstrap(dir, &["export", "--repo", GATEWAY]);
    assert_eq!(code, Some(1));
    let absent = format!(
        "meada: `{GATEWAY}` is not listed in the workspace manifest; it serves {WEB}, {WORKER}\n"
    );
    assert_eq!(stderr, absent);

    // GitHub now lists an accepted role for both, the custom one included.
    case.list("chain/gateway", &[("bia_acme", "release-manager")]);
    case.list("chain/catalog", &[("u0002", "admin"), ("caio", "read")]);
    let (_, stderr) = case.checked(BOOTSTRAP, None, "3 repositories, 5 rows, 0 exclusions");
    assert!(!stderr.contains("excluded"), "{stderr}");
    assert_eq!(case.excluded(), Some(format!("{version}\n")));
    let (shown, _, _) = on_bootstrap(dir, &["workspace", "show", "--pulled"]);
    assert!(
        shown.contains(GATEWAY) && shown.contains(CATALOG),
        "{shown}"
    );
    let (_, stderr, code) = on_bootstrap(dir, &["index"]);
    assert_eq!((stderr.as_str(), code), ("", Some(0)));
    assert!(indexed(GATEWAY) && indexed(CATALOG));
}

/// Every reader of the pulled version under `[oidc.github]` (`index`, `status`, `export`,
/// `snapshot publish` and `workspace show --pulled`) refuses, naming `meada github-app roles`,
/// at the first pull, with no exclusions file, after a new pull, with the file of the previous
/// version, and refuses a file with a malformed line naming the file and the line. With a file
/// of the pulled version, `index` indexes and records no connection on the loopback listeners of
/// the store, Valkey and Postgres. `--pulled` with a version is a usage error, and without
/// `[oidc.github]` it prints the pulled version whole. None of this needs a service.
#[test]
fn every_reader_refuses_a_missing_stale_or_malformed_exclusions_file() {
    let listeners: Vec<std::net::TcpListener> = (0..3)
        .map(|_| std::net::TcpListener::bind("127.0.0.1:0").unwrap())
        .collect();
    let port = |n: usize| listeners[n].local_addr().unwrap().port();
    let dir = chain_manifest("roles_readers");
    fs::write(dir.join("ops/s3-secret"), "x\n").unwrap();
    fs::write(
        dir.join("ops/pg-url"),
        format!("postgres://meada:pw@127.0.0.1:{}/meada", port(2)),
    )
    .unwrap();
    let github = "[oidc.github]\nhost = \"example.com\"\nlogin_claim = \"github_login\"\n\
                  max_age_seconds = 5400\n";
    let write = |github: &str| {
        let bootstrap = format!(
            "index_dir = \"../store-indexes\"\n{}\n[valkey]\nurl = \"redis://127.0.0.1:{}\"\n\n\
             [generations]\nkeep = 3\n\n[workspace]\nsource = \"store\"\n\n[postgres]\n\
             url_file = \"pg-url\"\n\n[oidc]\nissuer = \"https://id.example\"\npublic_url = \
             \"https://meada.example\"\nidentities_claim = \"r\"\n{github}",
            crate::snapshot::section(&format!("http://127.0.0.1:{}", port(0)), "offline"),
            port(1)
        );
        fs::write(dir.join(BOOTSTRAP), bootstrap).unwrap();
    };
    write(github);
    let _ = fs::remove_dir_all(dir.join("store-indexes"));
    let entries = chain([Some("ana-souza"), None, None, None]);
    let v1 = pull_version(&dir, &entries);
    let file = "ops/../store-indexes/workspace/excluded";
    let readers: [&[&str]; 5] = [
        &["index"],
        &["status"],
        &["export", "--repo", WEB],
        &["snapshot", "publish"],
        &["workspace", "show", "--pulled"],
    ];
    let refused = |expected: &str| {
        for args in readers {
            let (stdout, stderr, code) = on_bootstrap(&dir, args);
            assert_eq!((stdout.as_str(), code), ("", Some(1)), "{args:?}: {stderr}");
            assert_eq!(stderr, format!("meada: {expected}\n"), "{args:?}");
        }
    };
    refused(&format!(
        "{file} is missing: run `meada github-app roles` after `meada workspace pull`"
    ));
    let excluded = dir.join(PULLED).join("excluded");
    fs::write(&excluded, format!("{v1}\n{WEB}\n")).unwrap();
    let (shown, _, _) = on_bootstrap(&dir, &["workspace", "show", "--pulled"]);
    assert!(!shown.contains(WEB) && shown.contains(GATEWAY), "{shown}");
    // A new pull: the file names the version before.
    let v2 = pull_version(&dir, &entries[1..]);
    refused(&format!(
        "{file} names version {v1}, not the pulled version {v2}: run `meada github-app roles`"
    ));
    for (text, line) in [
        (format!("{v2}\n{WEB}\n../x\n"), 3),
        (format!("{v2}\n{WEB} \n"), 2),
        (format!("{v2}\n\n"), 2),
        (format!("not-an-id\n{WEB}\n"), 1),
        (format!("{v2}\nlocal/x\n"), 2),
        (String::new(), 1),
    ] {
        fs::write(&excluded, text).unwrap();
        refused(&format!(
            "{file}:{line}: not a version id or an identity: run `meada github-app roles`"
        ));
    }
    fs::write(&excluded, format!("{v2}\n")).unwrap();
    let (stdout, stderr, code) = on_bootstrap(&dir, &["index"]);
    assert_eq!((stdout.as_str(), stderr.as_str(), code), ("", "", Some(0)));
    for listener in &listeners {
        listener.set_nonblocking(true).unwrap();
        let accepted = listener.accept().map(drop).map_err(|e| e.kind());
        assert_eq!(accepted, Err(std::io::ErrorKind::WouldBlock));
    }
    let (_, stderr, code) = on_bootstrap(&dir, &["workspace", "show", "--pulled", &v2]);
    assert_eq!(code, Some(2), "{stderr}");
    assert!(stderr.contains("--pulled"), "{stderr}");
    // Without `[oidc.github]` the version is printed whole and no file is read.
    fs::remove_file(&excluded).unwrap();
    write("");
    let (shown, stderr, code) = on_bootstrap(&dir, &["workspace", "show", "--pulled"]);
    assert_eq!((stderr.as_str(), code), ("", Some(0)));
    assert!(
        shown.contains(GATEWAY) && shown.contains(WORKER) && !shown.contains(WEB),
        "{shown}"
    );
}

/// The local publication of a check (amendment of 2026-10-23): a failed check (a listing that
/// fails or Postgres unreachable), a commit whose exclusions file cannot be written and a check
/// killed mid-listing each leave no exclusions file, so a reader refuses rather than admit the previous check's exclusions for the same
/// version, and the next check that succeeds writes it. Two checks at once: a low fence held
/// mid-listing while a high one completes is refused at its commit, naming the high fence, and
/// leaves the high check's file as it wrote it.
#[cfg(feature = "refresh-lock")]
#[test]
fn a_failed_or_stale_check_never_leaves_exclusions_a_reader_admits() {
    let Some(case) = Case::new("roles_publication", &[1]) else {
        return;
    };
    let dir = &case.dir;
    case.bootstrap(None, true);
    let version = pull_version(dir, &chain([None, Some("bia_acme"), None, None]));
    case.list("chain/web", &[("u0001", "write")]);
    case.list("chain/gateway", &[("u0002", "write")]);
    case.list("chain/catalog", &[("u0003", "write")]);
    let excluding = format!("{version}\n{GATEWAY}\n");
    let refused = |dir: &Path| {
        let (_, stderr, code) = on_bootstrap(dir, &["index"]);
        assert_eq!(code, Some(1));
        assert!(
            stderr.contains("is missing: run `meada github-app roles`"),
            "{stderr}"
        );
    };
    let rows = "3 repositories, 3 rows, 1 exclusions";
    case.checked(BOOTSTRAP, Some(1), rows);
    assert_eq!(case.excluded().as_deref(), Some(excluding.as_str()));

    // A failed check: the file of the same version is gone.
    case.api.with(|s| s.listing_status = Some(500));
    assert_eq!(case.roles(BOOTSTRAP, Some(1)).2, Some(1));
    assert_eq!(case.excluded(), None);
    refused(dir);
    case.api.with(|s| s.listing_status = None);
    case.checked(BOOTSTRAP, Some(1), rows);
    assert_eq!(case.excluded().as_deref(), Some(excluding.as_str()));

    // Postgres unreachable fails the check all the same: the file is gone.
    let pg_url = dir.join("ops/pg-url");
    fs::write(&pg_url, "postgres://meada:closed@127.0.0.1:9/meada").unwrap();
    assert_eq!(case.roles(BOOTSTRAP, Some(1)).2, Some(1));
    assert_eq!(case.excluded(), None);
    refused(dir);
    fs::write(&pg_url, &case.url).unwrap();
    case.checked(BOOTSTRAP, Some(1), rows);
    assert_eq!(case.excluded().as_deref(), Some(excluding.as_str()));

    // The commit passes and the file cannot be written: exit 1 naming it, no file.
    let blocked = dir.join(PULLED).join("excluded.new");
    fs::create_dir_all(blocked.join("x")).unwrap();
    let (stdout, stderr, code) = case.roles(BOOTSTRAP, Some(2));
    assert_eq!((stdout.as_str(), code), ("", Some(1)));
    assert!(
        stderr.contains("store-indexes/workspace/excluded.new: "),
        "{stderr}"
    );
    assert_eq!(case.excluded(), None);
    refused(dir);
    fs::remove_dir_all(&blocked).unwrap();
    // The set it committed stands: a lower fence is refused, naming it.
    let (_, stderr, _) = case.roles(BOOTSTRAP, Some(1));
    assert!(
        stderr.contains("has fence 2, above this run's 1"),
        "{stderr}"
    );
    case.checked(BOOTSTRAP, Some(2), rows);

    // Killed mid-listing: no file until a check completes again.
    case.api.with(|s| s.hold_next = true);
    let mut held = spawn_roles(dir, 3);
    wait(|| case.api.with(|s| s.holding));
    held.kill().unwrap();
    held.wait().unwrap();
    case.api.with(|s| s.holding = false);
    assert_eq!(case.excluded(), None);
    refused(dir);
    case.checked(BOOTSTRAP, Some(3), rows);

    // A low fence held mid-listing while a high one completes, with GitHub then listing the
    // gateway's adder: the low check, which would exclude nothing, is refused at its commit.
    case.api.with(|s| s.hold_next = true);
    let low = spawn_roles(dir, 4);
    wait(|| case.api.with(|s| s.holding));
    case.list("chain/gateway", &[("u0002", "write"), ("bia_acme", "read")]);
    case.checked(BOOTSTRAP, Some(5), "3 repositories, 4 rows, 0 exclusions");
    let high = format!("{version}\n");
    assert_eq!(case.excluded().as_deref(), Some(high.as_str()));
    case.list("chain/gateway", &[("u0002", "write")]);
    case.api.with(|s| s.holding = false);
    let out = low.wait_with_output().unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    case.clean(&stderr);
    assert_eq!(out.status.code(), Some(1), "{stderr}");
    assert!(
        stderr.ends_with(
            "meada: the newest permissions set has fence 5, above this run's 4: a newer refresh \
         wrote it; nothing was written\n"
        ),
        "{stderr}"
    );
    assert_eq!(case.excluded().as_deref(), Some(high.as_str()));
}

/// `meada github-app roles --workspace ops/bootstrap.toml` at `fence`, started.
fn spawn_roles(dir: &Path, fence: u64) -> std::process::Child {
    let fence = fence.to_string();
    let args = ["github-app", "roles", "--workspace", BOOTSTRAP];
    (command(dir, &args, &[("MEADA_REFRESH_FENCE", &fence)]))
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("meada runs")
}

/// Waits up to 30 s for `done`.
fn wait(done: impl Fn() -> bool) {
    let start = Instant::now();
    while !done() {
        assert!(start.elapsed() < Duration::from_secs(30), "timed out");
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// `roles` exits 1 on a schema it does not know: one never migrated, naming `meada postgres
/// migrate`, and one a newer Meada migrated, naming both versions; neither lists anything.
#[test]
fn the_role_check_refuses_a_schema_version_it_does_not_know() {
    let Some(case) = Case::new("roles_schema", &[1]) else {
        return;
    };
    case.bootstrap(None, false);
    pull_version(&case.dir, &chain([None; 4]));
    let admin = crate::postgres::Admin::new(&case.url);
    admin.run(&format!("DROP SCHEMA {} CASCADE", case.schema));
    let (stdout, stderr, code) = case.roles(BOOTSTRAP, None);
    assert_eq!((stdout.as_str(), code), ("", Some(1)));
    assert!(stderr.contains("meada postgres migrate"), "{stderr}");
    let migrate = ["postgres", "migrate", "--workspace", BOOTSTRAP];
    assert_eq!(meada(&case.dir, &migrate, &[]).2, Some(0));
    let capped = [("MEADA_POSTGRES_VERSION_MAX", "4")];
    let (stdout, stderr, code) = meada(
        &case.dir,
        &["github-app", "roles", "--workspace", BOOTSTRAP],
        &capped,
    );
    case.clean(&stderr);
    assert_eq!((stdout.as_str(), code), ("", Some(1)));
    assert!(stderr.contains('5') && stderr.contains('4'), "{stderr}");
    assert!(case.listings().is_empty());
}

/// A `Link` header the role check refuses fails the run before any further request: another
/// origin, a user in it, another repository, a page listed already. The token goes nowhere else.
/// An answer past the size cap fails the run too.
#[test]
fn a_hostile_pagination_link_fails_the_run_and_is_never_followed() {
    let Some(case) = Case::new("roles_links", &[1]) else {
        return;
    };
    case.bootstrap(None, false);
    pull_version(&case.dir, &chain([None; 4]));
    case.list("chain/web", &[("u0001", "write")]);
    let foreign = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let elsewhere = format!("http://{}", foreign.local_addr().unwrap());
    let base = case.api.base.clone();
    let origin = base.trim_start_matches("http://");
    for (link, why) in [
        (
            format!("<{elsewhere}/repos/chain/web/collaborators?page=2>; rel=\"next\""),
            "foreign",
        ),
        (
            format!("<http://u:p@{origin}/repos/chain/web/collaborators?page=2>; rel=\"next\""),
            "foreign",
        ),
        (
            format!("<{base}/repos/chain/gateway/collaborators?page=2>; rel=\"next\""),
            "foreign",
        ),
        (
            format!("<{base}/repos/chain/web/collaborators?page=1>; rel=\"next\""),
            "cycle",
        ),
    ] {
        case.api.with(|s| s.link = Some(link.clone()));
        let before = case.listings().len();
        let (stdout, stderr, code) = case.roles(BOOTSTRAP, None);
        assert_eq!((stdout.as_str(), code), ("", Some(1)), "{link}");
        let reason = match why {
            "foreign" => "a Link header's next page is not this repository's collaborators listing",
            _ => "a Link header's next page was listed already",
        };
        assert_eq!(
            stderr,
            format!("meada: {WEB}: the role check failed: {reason}; nothing was written\n"),
            "{link}"
        );
        assert!(!stderr.contains("u:p"), "{stderr}");
        assert_eq!(case.listings().len(), before + 1, "{link}");
    }
    foreign.set_nonblocking(true).unwrap();
    let accepted = foreign.accept().map(drop).map_err(|e| e.kind());
    assert_eq!(accepted, Err(std::io::ErrorKind::WouldBlock));
    // An answer past the size cap of the module's REST answers (4 MiB) fails the run alike; one
    // at the cap is read.
    case.api.with(|s| {
        s.link = None;
        s.listing_bytes = Some((4 << 20) + 1);
    });
    let (stdout, stderr, code) = case.roles(BOOTSTRAP, None);
    assert_eq!((stdout.as_str(), code), ("", Some(1)));
    let failed = format!("meada: {WEB}: the role check failed: ");
    assert!(
        stderr.starts_with(&failed) && stderr.ends_with("; nothing was written\n"),
        "{stderr}"
    );
    case.api.with(|s| s.listing_bytes = Some(4 << 20));
    case.checked(BOOTSTRAP, None, "4 repositories, 0 rows, 0 exclusions");
}

/// The job's refresh under `meada snapshot lock` (pull, the role check, the fetch list
/// `show --pulled` gives, index, publish) against the local store: an addition without an
/// accepted role is absent from the published generation; one that stops at the role check
/// publishes nothing. `import` and `show` carry `added_by`, and the output imports to the same
/// id. Once the global operator clears the adder (adopting the entry), a refresh whose index
/// fails keeps the current generation (#316), and the next complete one publishes the entry in
/// a new generation. A change of `added_by` alone, eligibility unchanged, publishes the same
/// generation.
#[cfg(feature = "refresh-lock")]
#[test]
fn a_refresh_publishes_only_eligible_additions_and_an_adopted_one_after_its_check() {
    let Some(case) = Case::new("roles_refresh", &[1]) else {
        return;
    };
    let Some(fleet) = crate::refresh_lock::fleet("roles_refresh_fleet") else {
        return;
    };
    let dir = &fleet.dir;
    // The fleet's chain in `dir`, with its store, Valkey and lock; the role check's sections.
    fs::write(dir.join("ops/pg-url"), &case.url).unwrap();
    let manifest = fs::read_to_string(dir.join(MANIFEST)).unwrap();
    let fleet_sections = &manifest[manifest.find("\n[snapshot]").unwrap()..];
    let bootstrap = format!(
        "index_dir = \"../store-indexes\"\n{fleet_sections}\n[workspace]\nsource = \"store\"\n{}",
        case.sections(None, false)
    );
    fs::write(dir.join(BOOTSTRAP), bootstrap).unwrap();
    let _ = fs::remove_dir_all(dir.join("store-indexes"));
    assert_eq!(on_bootstrap(dir, &["postgres", "migrate"]).2, Some(0));
    // The fleet's Valkey holds the installations under its own prefix: the fake's App 1.
    case.list("chain/web", &[("ana-souza", "write")]);
    case.list("chain/gateway", &[("u0001", "write")]);
    case.list("chain/catalog", &[("u0002", "write")]);
    case.list("chain/worker", &[("u0003", "write")]);

    let text = fs::read_to_string(dir.join(MANIFEST)).unwrap();
    let version = |name: &str, logins: [Option<&str>; 4]| {
        let file = format!("ops/{name}.toml");
        fs::write(dir.join(&file), crate::generations::added(&text, &logins)).unwrap();
        let (id, _) = crate::generations::import(dir, &file, name, "alice");
        let (stdout, stderr, code) = meada(
            dir,
            &["workspace", "apply", &id, "--workspace", MANIFEST],
            &[],
        );
        assert_eq!(code, Some(0), "{stdout}{stderr}");
        id
    };
    let v1 = version("v1", [Some("ana-souza"), Some("bia_acme"), None, None]);
    // `show` carries `added_by`, and its output imports to the same id.
    let (shown, stderr, code) = meada(
        dir,
        &["workspace", "show", &v1, "--workspace", MANIFEST],
        &[],
    );
    assert_eq!(code, Some(0), "{stderr}");
    assert!(shown.contains("added_by = \"bia_acme\"\n"), "{shown}");
    fs::write(dir.join("ops/shown.toml"), &shown).unwrap();
    assert_eq!(
        crate::generations::import(dir, "ops/shown.toml", "again", "bob").0,
        v1
    );

    let s3 = crate::snapshot::bucket(&fleet.endpoint, &fleet.bucket);
    let held = |generation: &str| -> Vec<String> {
        let key = format!("prod/meada/generations/{generation}.json");
        let document: serde_json::Value =
            serde_json::from_slice(&crate::snapshot::get(&s3, &key)).unwrap();
        let repositories = document["repositories"].as_array().unwrap();
        let mut held: Vec<String> = (repositories.iter())
            .map(|r| r["identity"].as_str().unwrap().to_string())
            .collect();
        held.sort();
        held
    };
    let refresh = || {
        let script = "set -e\n\
            \"$MEADA\" workspace pull --workspace ops/bootstrap.toml\n\
            \"$MEADA\" github-app roles --workspace ops/bootstrap.toml\n\
            \"$MEADA\" workspace show --pulled --workspace ops/bootstrap.toml > ops/fetch.list\n\
            \"$MEADA\" index --workspace ops/bootstrap.toml\n\
            \"$MEADA\" snapshot publish --workspace ops/bootstrap.toml\n";
        let args = [
            "snapshot",
            "lock",
            "--workspace",
            BOOTSTRAP,
            "--",
            "sh",
            "-c",
            script,
        ];
        let (stdout, stderr, code) = meada(dir, &args, &[]);
        case.clean(&stderr);
        (
            stdout.lines().last().unwrap_or("").to_string(),
            stderr,
            code,
        )
    };
    let current = || crate::refresh_lock::current(&fleet).0;

    let (g1, stderr, code) = refresh();
    assert_eq!(code, Some(0), "{stderr}");
    assert!(
        stderr.contains(&format!(
            "meada: {GATEWAY}: excluded: GitHub lists no role for its added_by\n"
        )),
        "{stderr}"
    );
    assert_eq!(current(), g1);
    assert_eq!(held(&g1), [CATALOG, WEB, WORKER]);
    let list = fs::read_to_string(dir.join("ops/fetch.list")).unwrap();
    assert!(!list.contains(GATEWAY) && list.contains(WEB), "{list}");

    // A refresh that stops at the role check publishes nothing.
    case.api.with(|s| s.listing_status = Some(502));
    let (_, stderr, code) = refresh();
    assert_eq!(code, Some(1), "{stderr}");
    assert!(stderr.contains("the role check failed"), "{stderr}");
    assert_eq!(current(), g1);
    case.api.with(|s| s.listing_status = None);

    // Adopted: the gateway's adder cleared. Its index cannot be written: nothing is published.
    version("v2", [Some("ana-souza"), None, None, None]);
    let indexes = dir.join("store-indexes/example.com/chain");
    let Some(locked) = crate::manifest::ReadOnly::lock(&indexes) else {
        return;
    };
    let (_, stderr, code) = refresh();
    assert_eq!(code, Some(1), "{stderr}");
    assert!(stderr.contains("index failed"), "{stderr}");
    assert_eq!(current(), g1);
    drop(locked);
    let (g2, stderr, code) = refresh();
    assert_eq!(code, Some(0), "{stderr}");
    assert_ne!(g2, g1);
    assert_eq!(current(), g2);
    assert_eq!(held(&g2), [CATALOG, GATEWAY, WEB, WORKER]);

    // Attribution alone changes: the same eligible entries, commits and blocks, the same
    // generation.
    case.list("chain/catalog", &[("u0002", "write"), ("caio", "read")]);
    version("v3", [Some("ana-souza"), None, Some("caio"), None]);
    let (g3, stderr, code) = refresh();
    assert_eq!(code, Some(0), "{stderr}");
    assert_eq!(g3, g2);
}
