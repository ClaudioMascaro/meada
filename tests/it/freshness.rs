//! Freshness (ADR 0004): every query first brings the index up to date with the files on
//! disk, and a refreshed index exports exactly what a cold index of the same files does.

use crate::common;

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use common::{
    age, assert_golden, chain, documented_chain, drift_chain, fixture_repo, set_mtime, worker,
};
use meada::engine::Engine;

const ORIGIN: Option<&str> = Some("https://example.com/acme/conformance.git");
const WEB: Option<&str> = Some("https://example.com/acme/web.git");

/// Edits a file whose exports others import, adds a file whose definition turns an inferred
/// call in an unrelated file into an ambiguous one, deletes a file, and drops the tsconfig
/// path alias.
fn edit(repo: &Path) {
    let math = fs::read_to_string(repo.join("src/math.ts")).unwrap();
    fs::write(
        repo.join("src/math.ts"),
        math.replace("export const square", "const square"),
    )
    .unwrap();
    fs::write(
        repo.join("src/extra.ts"),
        "export function legacy() {\n  return 2;\n}\n",
    )
    .unwrap();
    fs::remove_file(repo.join("src/widget.jsx")).unwrap();
    fs::write(repo.join("tsconfig.json"), "{}\n").unwrap();
}

fn cold_export(case: &str, fixture: &str, origin: Option<&str>, edit: impl Fn(&Path)) -> String {
    let repo = fixture_repo(case, fixture, origin);
    edit(&repo);
    Engine::open(&repo).unwrap().export().unwrap()
}

#[test]
fn refreshed_export_is_byte_identical_to_a_cold_export_of_the_same_files() {
    let repo = fixture_repo("fresh_incremental", "conformance-ts-js", ORIGIN);
    age(&repo);
    let before = Engine::open(&repo).unwrap().export().unwrap();
    edit(&repo);

    let refreshed = Engine::open(&repo).unwrap().export().unwrap();

    assert_ne!(refreshed, before);
    assert_eq!(
        refreshed,
        cold_export("fresh_incremental_cold", "conformance-ts-js", ORIGIN, edit)
    );
}

/// Renames a function others import, adds a module whose function turns an inferred call
/// into an ambiguous one, deletes a module star-imported elsewhere, and declares a source
/// root, which drops the implicit `src` one.
fn edit_python(repo: &Path) {
    let units = fs::read_to_string(repo.join("geometry/units.py")).unwrap();
    fs::write(
        repo.join("geometry/units.py"),
        units.replace("def to_cm", "def to_mm"),
    )
    .unwrap();
    fs::write(repo.join("stars/extra.py"), "def legacy():\n    return 2\n").unwrap();
    fs::remove_file(repo.join("helpers.py")).unwrap();
    fs::write(
        repo.join("pyproject.toml"),
        "[tool.setuptools.packages.find]\nwhere = [\"lib\"]\n",
    )
    .unwrap();
}

#[test]
fn python_refreshed_export_is_byte_identical_to_a_cold_export_of_the_same_files() {
    let repo = fixture_repo("fresh_python", "conformance-python", ORIGIN);
    age(&repo);
    let before = Engine::open(&repo).unwrap().export().unwrap();
    edit_python(&repo);

    let refreshed = Engine::open(&repo).unwrap().export().unwrap();

    assert_ne!(refreshed, before);
    assert_eq!(
        refreshed,
        cold_export(
            "fresh_python_cold",
            "conformance-python",
            ORIGIN,
            edit_python
        )
    );
}

/// Drops the console script that names `reports:build` and adds one for `jobs.steps:store`.
fn edit_python_scripts(repo: &Path) {
    fs::write(
        repo.join("pyproject.toml"),
        "[project]\nname = \"acme-conformance\"\n[project.scripts]\n\
         acme-sync = \"jobs.sync:run\"\nacme-steps = \"jobs.steps:store\"\n",
    )
    .unwrap();
}

/// The kind of the process an entry point of the export starts, by the end of its identity.
fn kind_of(export: &str, entry_point: &str) -> Option<String> {
    let json: serde_json::Value = serde_json::from_str(export).unwrap();
    let processes = json["processes"].as_array().unwrap().iter();
    let mut kinds = processes.filter(|p| p["entry_point"].as_str().unwrap().ends_with(entry_point));
    kinds
        .next()
        .map(|p| p["kind"].as_str().unwrap().to_string())
}

#[test]
fn python_console_scripts_follow_an_edited_pyproject_like_a_cold_index() {
    let repo = fixture_repo("fresh_python_scripts", "conformance-python", ORIGIN);
    age(&repo);
    let before = Engine::open(&repo).unwrap().export().unwrap();
    edit_python_scripts(&repo);

    let refreshed = Engine::open(&repo).unwrap().export().unwrap();

    // `build` is a program while a script names it, an export once none does; `store` is
    // called by other functions, so only a script makes it a door.
    assert_eq!(
        kind_of(&before, "reports/__init__.py::build").as_deref(),
        Some("program")
    );
    assert_eq!(
        kind_of(&refreshed, "reports/__init__.py::build").as_deref(),
        Some("export")
    );
    assert_eq!(kind_of(&before, "jobs/steps.py::store"), None);
    assert_eq!(
        kind_of(&refreshed, "jobs/steps.py::store").as_deref(),
        Some("program")
    );
    assert_eq!(
        refreshed,
        cold_export(
            "fresh_python_scripts_cold",
            "conformance-python",
            ORIGIN,
            edit_python_scripts
        )
    );
}

/// Renames a topic constant the Python channel files import, adds a consumer of a topic that
/// had none, and deletes a module with producers and consumers (spec #73).
fn edit_python_channels(repo: &Path) {
    let topics = fs::read_to_string(repo.join("messages/topics.py")).unwrap();
    assert!(topics.contains("\"orders\""));
    fs::write(
        repo.join("messages/topics.py"),
        topics.replace("\"orders\"", "\"sales\""),
    )
    .unwrap();
    fs::write(
        repo.join("messages/extra.py"),
        "from confluent_kafka import Consumer\n\n\ndef consume_extra():\n    \
         consumer = Consumer({})\n    consumer.subscribe([\"retries\"])\n",
    )
    .unwrap();
    fs::remove_file(repo.join("messages/sqs_boto.py")).unwrap();
}

#[test]
fn python_channels_follow_an_edited_topic_constant_like_a_cold_index() {
    let repo = fixture_repo("fresh_python_channels", "conformance-python", ORIGIN);
    age(&repo);
    let before = Engine::open(&repo).unwrap().export().unwrap();
    edit_python_channels(&repo);

    let refreshed = Engine::open(&repo).unwrap().export().unwrap();

    assert_ne!(refreshed, before);
    // The renamed constant reaches the channels of the files that import it.
    assert!(refreshed.contains("channel::kafka::sales"));
    assert!(!refreshed.contains("channel::kafka::orders"));
    assert_eq!(
        refreshed,
        cold_export(
            "fresh_python_channels_cold",
            "conformance-python",
            ORIGIN,
            edit_python_channels
        )
    );
}

/// Moves `mod health;` out of `routes/mod.rs` (its file is no longer reached), declares the
/// unreached `orphan.rs`, adds a module with its file, deletes a module's file, edits a
/// function others call and drops the `[[bin]]` root from a member's `Cargo.toml`.
fn edit_rust(repo: &Path) {
    let replace = |file: &str, from: &str, to: &str| {
        let text = fs::read_to_string(repo.join(file)).unwrap();
        assert!(text.contains(from), "{file} holds {from}");
        fs::write(repo.join(file), text.replacen(from, to, 1)).unwrap();
    };
    replace("api/src/routes/mod.rs", "mod health;\n", "");
    replace(
        "api/src/main.rs",
        "mod routes;\n",
        "mod routes;\nmod health;\nmod orphan;\n",
    );
    replace("api/src/store.rs", "pub fn reset()", "pub fn restart()");
    replace(
        "crates/billing/src/billing.rs",
        "pub mod invoice;\n",
        "pub mod invoice;\nmod audit;\n",
    );
    fs::write(
        repo.join("crates/billing/src/audit.rs"),
        "pub fn record() -> u32 {\n    crate::total()\n}\n",
    )
    .unwrap();
    fs::remove_file(repo.join("api/tools/shared.rs")).unwrap();
    replace("api/Cargo.toml", "[[bin]]", "[[example]]");
    // A `pub use` edited, an item moved to another file, a package renamed.
    replace(
        "crates/billing/src/billing.rs",
        "pub use invoice::Invoice;",
        "pub use invoice::{tax, Invoice};",
    );
    let lib = "crates/ledger-core/src/lib.rs";
    replace(lib, "mod meter;\n\n", "");
    replace(lib, "pub use meter::meter;", "pub fn meter() {}");
    fs::remove_file(repo.join("crates/ledger-core/src/meter.rs")).unwrap();
    replace(
        "crates/ledger-core/Cargo.toml",
        "name = \"ledger-core\"",
        "name = \"ledger-kernel\"",
    );
}

#[test]
fn rust_refreshed_export_is_byte_identical_to_a_cold_export_of_the_same_files() {
    let repo = fixture_repo("fresh_rust", "conformance-rust", ORIGIN);
    age(&repo);
    let before = Engine::open(&repo).unwrap().export().unwrap();
    edit_rust(&repo);

    let refreshed = Engine::open(&repo).unwrap().export().unwrap();

    // The moved declaration looks for `api/src/health.rs`, which is not there.
    assert!(before.contains("api/src/routes/health.rs::check"));
    assert!(!refreshed.contains("api/src/routes/health.rs"));
    assert!(refreshed.contains("api/src/orphan.rs::lost"));
    assert!(refreshed.contains("crates/billing/src/audit.rs::record"));
    assert!(!refreshed.contains("api/tools/admin.rs"));
    assert!(before.contains("crates/ledger-core/src/meter.rs::meter"));
    assert!(!refreshed.contains("crates/ledger-core/src/meter.rs"));
    assert_eq!(
        refreshed,
        cold_export("fresh_rust_cold", "conformance-rust", ORIGIN, edit_rust)
    );
}

fn enable_metrics(repo: &Path) {
    let manifest = repo.join("api/Cargo.toml");
    let text = fs::read_to_string(&manifest).unwrap();
    let from = "default = [\"net\", \"json\"]";
    assert!(text.contains(from));
    let to = "default = [\"net\", \"json\", \"metrics\"]";
    fs::write(manifest, text.replacen(from, to, 1)).unwrap();
}

#[test]
fn a_default_feature_toggled_in_cargo_toml_refreshes_to_the_cold_export() {
    let repo = fixture_repo("fresh_rust_feature", "conformance-rust", ORIGIN);
    age(&repo);
    let before = Engine::open(&repo).unwrap().export().unwrap();
    enable_metrics(&repo);

    let refreshed = Engine::open(&repo).unwrap().export().unwrap();

    assert!(!before.contains("api/src/metrics.rs"));
    assert!(!before.contains("gated.rs::metrics_only"));
    assert!(before.contains("gated.rs::unix_without_metrics"));
    assert!(refreshed.contains("api/src/metrics.rs::gauge"));
    assert!(refreshed.contains("gated.rs::metrics_only"));
    assert!(refreshed.contains("gated.rs::Gauge.sampled"));
    assert!(!refreshed.contains("gated.rs::unix_without_metrics"));
    assert_eq!(
        refreshed,
        cold_export(
            "fresh_rust_feature_cold",
            "conformance-rust",
            ORIGIN,
            enable_metrics
        )
    );
}

fn add_bin(repo: &Path) {
    fs::write(repo.join("api/tools/second.rs"), "fn main() {}\n").unwrap();
    let manifest = repo.join("api/Cargo.toml");
    let mut text = fs::read_to_string(&manifest).unwrap();
    text.push_str("\n[[bin]]\nname = \"second\"\npath = \"tools/second.rs\"\n");
    fs::write(manifest, text).unwrap();
}

#[test]
fn a_bin_added_to_cargo_toml_refreshes_its_program_to_the_cold_export() {
    let repo = fixture_repo("fresh_rust_bin", "conformance-rust", ORIGIN);
    age(&repo);
    let before = Engine::open(&repo).unwrap().export().unwrap();
    add_bin(&repo);

    let refreshed = Engine::open(&repo).unwrap().export().unwrap();

    assert!(!before.contains("api/tools/second.rs"));
    assert!(refreshed.contains("example.com/acme/conformance::api/tools/second.rs::main"));
    assert_eq!(
        refreshed,
        cold_export("fresh_rust_bin_cold", "conformance-rust", ORIGIN, add_bin)
    );
}

#[test]
fn an_open_engine_answers_after_files_change() {
    // No aging: every file was just written, so none can be trusted by mtime alone.
    let repo = fixture_repo("fresh_same_engine", "conformance-ts-js", ORIGIN);
    let engine = Engine::open(&repo).unwrap();
    engine.export().unwrap();
    edit(&repo);

    let refreshed = engine.export().unwrap();

    assert_eq!(
        refreshed,
        cold_export("fresh_same_engine_cold", "conformance-ts-js", ORIGIN, edit)
    );
}

#[test]
fn unchanged_files_are_not_read_again() {
    // An unreadable file whose mtime and size still match the index cannot be read, so a
    // refresh that succeeds took its facts from the index.
    let view = |repo: &Path| {
        fs::write(
            repo.join("src/view.tsx"),
            "function label() {\n  add();\n}\n",
        )
        .unwrap()
    };
    let repo = fixture_repo("fresh_unread", "ts-basic", WEB);
    age(&repo);
    let engine = Engine::open(&repo).unwrap();
    engine.export().unwrap();
    let math = repo.join("src/math.ts");
    fs::set_permissions(&math, fs::Permissions::from_mode(0o000)).unwrap();
    view(&repo);

    let refreshed = engine.export();

    fs::set_permissions(&math, fs::Permissions::from_mode(0o644)).unwrap();
    assert_eq!(
        refreshed.unwrap(),
        cold_export("fresh_unread_cold", "ts-basic", WEB, view)
    );
}

/// The two-repository fixture as siblings in one case directory.
fn web_and_api(case: &str) -> (PathBuf, PathBuf) {
    (
        fixture_repo(case, "http-web", Some("git@example.com:acme/web.git")),
        fixture_repo(case, "http-api", Some("https://example.com/acme/api.git")),
    )
}

/// Changes links from both sides: the api drops one route handler, deletes a route and adds
/// one; the web consumes the new route.
fn edit_link_ends(web: &Path, api: &Path) {
    let users = api.join("app/api/users/route.ts");
    let route = fs::read_to_string(&users).unwrap();
    let route = route.split("export async function POST").next().unwrap();
    fs::write(&users, route).unwrap();
    fs::remove_file(api.join("app/api/health/route.js")).unwrap();
    fs::create_dir_all(api.join("app/api/teams")).unwrap();
    fs::write(
        api.join("app/api/teams/route.ts"),
        "export async function GET() {\n  return Response.json([]);\n}\n",
    )
    .unwrap();
    fs::write(
        web.join("src/teams.ts"),
        "export async function listTeams() {\n  return fetch(\"https://api.example.com/api/teams\");\n}\n",
    )
    .unwrap();
}

#[test]
fn links_that_touch_changed_files_follow_a_refresh_on_both_sides() {
    let (web, api) = web_and_api("fresh_links");
    age(&web);
    let engine = Engine::open(&web).unwrap();
    let before = engine.export().unwrap();
    // The api has its own index already, refreshed from its side.
    Engine::open(&api).unwrap().export().unwrap();
    edit_link_ends(&web, &api);

    let consumer = engine.export().unwrap();
    let provider = Engine::open(&api).unwrap().export().unwrap();

    let (cold_web, cold_api) = web_and_api("fresh_links_cold");
    edit_link_ends(&cold_web, &cold_api);
    assert_ne!(consumer, before);
    assert_eq!(consumer, Engine::open(&cold_web).unwrap().export().unwrap());
    let (cold_web, cold_api) = web_and_api("fresh_links_cold_api");
    edit_link_ends(&cold_web, &cold_api);
    assert_eq!(provider, Engine::open(&cold_api).unwrap().export().unwrap());
}

#[test]
fn context_refreshes_when_the_span_hash_no_longer_matches() {
    // Same size and the old mtime: only the span hash shows the edit.
    let repo = fixture_repo("fresh_context", "ts-basic", WEB);
    age(&repo);
    let engine = Engine::open(&repo).unwrap();
    engine.context("add", None).unwrap();
    let path = repo.join("src/math.ts");
    let mtime = fs::metadata(&path).unwrap().modified().unwrap();
    let math = fs::read_to_string(&path).unwrap();
    fs::write(&path, math.replace("return a + b;", "return a * b;")).unwrap();
    set_mtime(&path, mtime);

    assert_golden(
        "ts-basic.context-add-edited.txt",
        &engine.context("add", None).unwrap(),
    );
}

/// Repositories whose contracts read values from other files: path constants (`http-paths`,
/// with its provider `http-api`), client wrappers (`http-wrappers`) and Express mounts
/// (`http-express`, whose inline handlers are named by the mounts above them).
fn values(case: &str) -> [PathBuf; 3] {
    fixture_repo(case, "http-api", Some("https://example.com/acme/api.git"));
    [
        fixture_repo(
            case,
            "http-paths",
            Some("https://example.com/acme/paths.git"),
        ),
        fixture_repo(
            case,
            "http-wrappers",
            Some("https://example.com/acme/admin.git"),
        ),
        fixture_repo(
            case,
            "http-express",
            Some("https://example.com/acme/backend.git"),
        ),
    ]
}

/// Edits only the files that hold values: a constant other files import, a wrapper's default
/// method, and the mount prefix that names the inline handlers of another file.
fn edit_values([paths, admin, backend]: &[PathBuf; 3]) {
    let replace = |path: PathBuf, from: &str, to: &str| {
        let src = fs::read_to_string(&path).unwrap();
        assert!(src.contains(from), "{from} in {}", path.display());
        fs::write(&path, src.replace(from, to)).unwrap();
    };
    replace(
        paths.join("src/routes.ts"),
        "\"/api/users\"",
        "\"/api/v2/users\"",
    );
    replace(
        admin.join("src/http.ts"),
        "method = \"GET\"",
        "method = \"POST\"",
    );
    replace(backend.join("src/server.ts"), "\"/api\"", "\"/v1\"");
}

#[test]
fn contracts_follow_values_edited_in_other_files() {
    let repos = values("fresh_values");
    let exports = |repos: &[PathBuf; 3]| -> Vec<String> {
        repos
            .iter()
            .map(|r| Engine::open(r).unwrap().export().unwrap())
            .collect()
    };
    repos.iter().for_each(|r| age(r));
    let before = exports(&repos);
    edit_values(&repos);

    let refreshed = exports(&repos);

    let cold = values("fresh_values_cold");
    edit_values(&cold);
    for (n, ((refreshed, before), cold)) in refreshed
        .iter()
        .zip(&before)
        .zip(exports(&cold))
        .enumerate()
    {
        assert_ne!(refreshed, before, "repository {n}");
        assert_eq!(*refreshed, cold, "repository {n}");
    }
}

/// Renames the topic the gateway produces through its wrapper, in the imported constant, and
/// the topics the conformance fixture builds from a constant (spec #20).
fn edit_topics(gateway: &Path, conformance: &Path) {
    let replace = |path: PathBuf, from: &str, to: &str| {
        let src = fs::read_to_string(&path).unwrap();
        assert!(src.contains(from), "{from} in {}", path.display());
        fs::write(&path, src.replace(from, to)).unwrap();
    };
    replace(
        gateway.join("src/topics.ts"),
        "orders.created",
        "orders.placed",
    );
    replace(
        conformance.join("src/messages/topics.ts"),
        "\"orders\"",
        "\"sales\"",
    );
}

#[test]
fn channels_follow_an_edited_topic_constant() {
    let topics = |case: &str| {
        let (_, gateway, _) = chain(case);
        let conformance = fixture_repo(&format!("{case}_conformance"), "conformance-ts-js", ORIGIN);
        [gateway.clone(), worker(&gateway), conformance]
    };
    let exports = |repos: &[PathBuf; 3]| -> Vec<String> {
        (repos.iter())
            .map(|r| Engine::open(r).unwrap().export().unwrap())
            .collect()
    };
    let repos = topics("fresh_topics");
    repos.iter().for_each(|r| age(r));
    let before = exports(&repos);
    edit_topics(&repos[0], &repos[2]);

    let refreshed = exports(&repos);

    let cold = topics("fresh_topics_cold");
    edit_topics(&cold[0], &cold[2]);
    for (n, ((refreshed, before), cold)) in refreshed
        .iter()
        .zip(&before)
        .zip(exports(&cold))
        .enumerate()
    {
        assert_ne!(refreshed, before, "repository {n}");
        assert_eq!(*refreshed, cold, "repository {n}");
    }
}

type Chain = (PathBuf, PathBuf, PathBuf);

/// Edits, adds and deletes files in each repository of the chain: the web adds a consumer and
/// deletes the file of two others, the gateway moves a route to another method, the catalog
/// deletes a route its peers link to and edits the end of its cycle.
fn edit_chain((web, gateway, catalog): &Chain) {
    let replace = |path: PathBuf, from: &str, to: &str| {
        let src = fs::read_to_string(&path).unwrap();
        assert!(src.contains(from), "{from} in {}", path.display());
        fs::write(&path, src.replace(from, to)).unwrap();
    };
    fs::write(
        web.join("src/archive.ts"),
        "import { request } from \"./api/client\";\n\n\
         export function archive(id: string) {\n  \
           return request(`/api/orders/${id}`, \"PATCH\");\n}\n",
    )
    .unwrap();
    fs::remove_file(web.join("src/status.ts")).unwrap();
    replace(gateway.join("src/routes.ts"), "api.delete(", "api.patch(");
    fs::remove_file(catalog.join("app/api/health/route.ts")).unwrap();
    replace(
        catalog.join("lib/products.ts"),
        "return { id };",
        "return { id, found: true };",
    );
}

#[test]
fn a_refreshed_chain_exports_what_a_cold_chain_does() {
    let exports = |(web, gateway, catalog): &Chain| -> Vec<String> {
        [web, gateway, catalog]
            .iter()
            .map(|r| Engine::open(r).unwrap().export().unwrap())
            .collect()
    };
    let repos = chain("fresh_chain");
    [&repos.0, &repos.1, &repos.2].iter().for_each(|r| age(r));
    let before = exports(&repos);
    edit_chain(&repos);

    let refreshed = exports(&repos);

    let cold = chain("fresh_chain_cold");
    edit_chain(&cold);
    for (n, ((refreshed, before), cold)) in refreshed
        .iter()
        .zip(&before)
        .zip(exports(&cold))
        .enumerate()
    {
        assert_ne!(refreshed, before, "repository {n}");
        assert_eq!(*refreshed, cold, "repository {n}");
    }
}

/// Moves the gateway's write of `orders` to `listOrders` and makes the worker a writer too.
fn edit_resources(gateway: &Path) {
    let edit = |path: PathBuf, from: &str, to: &str| {
        let src = fs::read_to_string(&path).unwrap();
        assert!(src.contains(from), "{from} in {}", path.display());
        fs::write(&path, src.replace(from, to)).unwrap();
    };
    edit(
        gateway.join("meada.toml"),
        "src/orders.ts::cancelOrder",
        "src/orders.ts::listOrders",
    );
    edit(
        worker(gateway).join("meada.toml"),
        "role = \"read\"",
        "role = \"read-write\"",
    );
}

/// Edits, adds and deletes `.py` files of the inventory and deletes a route of the storefront:
/// a route path changes, a module with a consumer on the declared host appears, the jobs
/// module goes, and the topic its handler consumes changes names.
fn edit_xlang((storefront, inventory): &(PathBuf, PathBuf)) {
    let main = inventory.join("inventory/main.py");
    let src = fs::read_to_string(&main).unwrap();
    assert!(src.contains("\"/stock/{sku}\""));
    fs::write(
        &main,
        src.replace("\"/stock/{sku}\"", "\"/stock/{sku}/level\""),
    )
    .unwrap();
    fs::write(
        inventory.join("inventory/report.py"),
        "import requests\n\n\ndef report():\n    return requests.post(\"https://storefront.example.com/api/audit\")\n",
    )
    .unwrap();
    fs::remove_file(inventory.join("inventory/jobs.py")).unwrap();
    fs::remove_file(storefront.join("app/api/promotions/route.ts")).unwrap();
    // The topic the storefront produces and the inventory's handler consumes changes names.
    let events = inventory.join("inventory/events.py");
    let src = fs::read_to_string(&events).unwrap();
    assert!(src.contains("stock.restocked"));
    fs::write(&events, src.replace("stock.restocked", "stock.replenished")).unwrap();
}

#[test]
fn a_refreshed_cross_language_pair_exports_what_a_cold_pair_does() {
    let exports = |(storefront, inventory): &(PathBuf, PathBuf)| -> Vec<String> {
        [storefront, inventory]
            .iter()
            .map(|r| Engine::open(r).unwrap().export().unwrap())
            .collect()
    };
    let repos = common::xlang("fresh_xlang");
    [&repos.0, &repos.1].iter().for_each(|r| age(r));
    let before = exports(&repos);
    edit_xlang(&repos);

    let refreshed = exports(&repos);

    let cold = common::xlang("fresh_xlang_cold");
    edit_xlang(&cold);
    for (n, ((refreshed, before), cold)) in refreshed
        .iter()
        .zip(&before)
        .zip(exports(&cold))
        .enumerate()
    {
        assert_ne!(refreshed, before, "repository {n}");
        assert_eq!(*refreshed, cold, "repository {n}");
    }
}

#[test]
fn an_edited_meada_toml_relinks_resources_like_a_cold_chain() {
    let exports = |gateway: &Path| -> Vec<String> {
        [gateway.to_path_buf(), worker(gateway)]
            .iter()
            .map(|r| Engine::open(r).unwrap().export().unwrap())
            .collect()
    };
    let (_, gateway, _) = chain("fresh_resources");
    let before = exports(&gateway);
    edit_resources(&gateway);

    let refreshed = exports(&gateway);

    let (_, cold, _) = chain("fresh_resources_cold");
    edit_resources(&cold);
    for (n, ((refreshed, before), cold)) in refreshed
        .iter()
        .zip(&before)
        .zip(exports(&cold))
        .enumerate()
    {
        assert_ne!(refreshed, before, "repository {n}");
        assert_eq!(*refreshed, cold, "repository {n}");
    }
}

/// Moves the documented-only route of the gateway's OpenAPI document, adds a JSON document with
/// a route of its own, and deletes the invalid one.
fn edit_documents(gateway: &Path) {
    let doc = fs::read_to_string(gateway.join("openapi.yaml")).unwrap();
    let doc = doc.replace("/api/shipments/{id}", "/api/parcels/{id}");
    fs::write(gateway.join("openapi.yaml"), doc).unwrap();
    fs::write(
        gateway.join("extra.openapi.json"),
        r#"{"openapi": "3.0.3", "paths": {"/api/returns": {"post": {}}}}"#,
    )
    .unwrap();
    fs::remove_file(gateway.join("broken.openapi.yaml")).unwrap();
}

/// Writes the listed path that did not exist, and takes the gitignored document and the
/// oddly named one off the list.
fn edit_listed_documents(gateway: &Path) {
    fs::write(
        gateway.join("docs/missing.yaml"),
        "openapi: 3.0.3\npaths:\n  /api/returns:\n    post: {}\n",
    )
    .unwrap();
    let toml = fs::read_to_string(gateway.join("meada.toml")).unwrap();
    let toml = toml.replace(
        "\"generated/openapi.yaml\", \"docs/api-contract.yaml\", ",
        "",
    );
    fs::write(gateway.join("meada.toml"), toml).unwrap();
}

/// Drops the gateway's `[serve]` table: its documents stop counting.
fn drop_serve(gateway: &Path) {
    let toml = fs::read_to_string(gateway.join("meada.toml")).unwrap();
    let serve = ["[serve]", "hosts = ", "documents = "];
    let kept = toml
        .lines()
        .filter(|l| !serve.iter().any(|k| l.starts_with(k)));
    let toml = kept.map(|l| format!("{l}\n")).collect::<String>();
    fs::write(gateway.join("meada.toml"), toml).unwrap();
}

/// The web's client wrapper returns the response instead of its parsed body: its caller, in an
/// unchanged file, reads no field any more.
fn return_the_response(web: &Path) {
    let path = web.join("src/api/body.ts");
    let src = fs::read_to_string(&path).unwrap();
    fs::write(&path, src.replace("return res.json();", "return res;")).unwrap();
}

#[test]
fn an_edited_client_wrapper_changes_its_callers_drift_like_a_cold_chain() {
    let (web, _, _) = drift_chain("fresh_drift");
    age(web.parent().unwrap());
    let before = Engine::open(&web).unwrap().export().unwrap();
    return_the_response(&web);

    let refreshed = Engine::open(&web).unwrap().export().unwrap();

    let (cold, _, _) = drift_chain("fresh_drift_cold");
    return_the_response(&cold);
    assert!(before.contains("\"[].customer\""));
    assert!(!refreshed.contains("\"[].customer\""));
    assert_eq!(refreshed, Engine::open(&cold).unwrap().export().unwrap());
}

/// The web's shared `Entity` type renames `id`: the consumers that extend it, in another file
/// and unchanged, declare `key` instead.
fn rename_a_shared_field(web: &Path) {
    let path = web.join("src/api/types.ts");
    let src = fs::read_to_string(&path).unwrap();
    let edited = src.replace("  id: string;", "  key: string;");
    assert_ne!(src, edited);
    fs::write(&path, edited).unwrap();
}

#[test]
fn an_edited_shared_type_changes_the_drift_of_its_users_like_a_cold_chain() {
    let (web, _, _) = drift_chain("fresh_type");
    age(web.parent().unwrap());
    let before = Engine::open(&web).unwrap().export().unwrap();
    rename_a_shared_field(&web);

    let refreshed = Engine::open(&web).unwrap().export().unwrap();

    let (cold, _, _) = drift_chain("fresh_type_cold");
    rename_a_shared_field(&cold);
    assert!(!before.contains("\"items[].key\": \"exact\""));
    assert!(refreshed.contains("\"items[].key\": \"exact\""));
    assert_eq!(refreshed, Engine::open(&cold).unwrap().export().unwrap());
}

/// The catalog's zod schema drops `title`, which a consumer in the web app reads.
fn drop_a_schema_field(catalog: &Path) {
    let path = catalog.join("lib/schemas.ts");
    let src = fs::read_to_string(&path).unwrap();
    let edited = src.replace("  title: z.string().min(1),\n", "");
    assert_ne!(src, edited);
    fs::write(&path, edited).unwrap();
}

#[test]
fn a_field_removed_from_a_zod_schema_adds_drift_like_a_cold_chain() {
    let exports = |repos: [&Path; 2]| -> Vec<String> {
        (repos.iter())
            .map(|r| Engine::open(r).unwrap().export().unwrap())
            .collect()
    };
    let (web, _, catalog) = drift_chain("fresh_zod");
    age(web.parent().unwrap());
    let before = exports([&catalog, &web]);
    drop_a_schema_field(&catalog);

    let refreshed = exports([&catalog, &web]);

    let (cold_web, _, cold) = drift_chain("fresh_zod_cold");
    drop_a_schema_field(&cold);
    assert!(!before[1].contains("\"[].title\": \"inferred\""));
    assert!(refreshed[1].contains("\"[].title\": \"inferred\""));
    assert_eq!(refreshed, exports([&cold, &cold_web]));
}

/// The catalog's shared package drops `price` from its zod schema and `warehouse` from its
/// constant, which consumers in the web app read; the routes importing them are unchanged.
fn drop_shared_fields(catalog: &Path) {
    for (file, from, to) in [
        ("quote.ts", ", price: z.number()", ""),
        (
            "shelf.ts",
            "{ warehouse: \"w1\", reserved: 0 }",
            "{ reserved: 0 }",
        ),
    ] {
        let path = catalog.join("packages/contracts/src").join(file);
        let src = fs::read_to_string(&path).unwrap();
        let edited = src.replace(from, to);
        assert_ne!(src, edited);
        fs::write(&path, edited).unwrap();
    }
}

#[test]
fn an_edited_shared_package_changes_its_importers_drift_like_a_cold_chain() {
    let exports = |repos: [&Path; 2]| -> Vec<String> {
        (repos.iter())
            .map(|r| Engine::open(r).unwrap().export().unwrap())
            .collect()
    };
    let (web, gateway, catalog) = drift_chain("fresh_shared");
    age(web.parent().unwrap());
    let before = exports([&gateway, &web]);
    drop_shared_fields(&catalog);

    let refreshed = exports([&gateway, &web]);

    let (cold_web, cold_gateway, cold) = drift_chain("fresh_shared_cold");
    drop_shared_fields(&cold);
    // The links of the quote's reader and the shelf's now miss what the package dropped.
    let link = |export: &str, consumer: &str| -> String {
        let site = format!("\"consumer\": \"example.com/chain/web::src/shared.ts::{consumer}\"");
        let at = export.find(&site).unwrap();
        export[at..].split("\n    }").next().unwrap().to_string()
    };
    for (consumer, missing) in [
        ("quote::fetch#0", "\"price\": \"inferred\""),
        ("shelf::fetch#0", "\"warehouse\": \"inferred\""),
    ] {
        assert!(!link(&before[1], consumer).contains(missing), "{missing}");
        assert!(link(&refreshed[1], consumer).contains(missing), "{missing}");
    }
    assert_eq!(refreshed, exports([&cold_gateway, &cold_web]));
}

#[test]
fn a_package_that_gets_its_name_back_keeps_its_shapes_like_a_cold_chain() {
    let (web, _, catalog) = drift_chain("fresh_shared_name");
    let path = catalog.join("packages/contracts/package.json");
    let named = fs::read_to_string(&path).unwrap();
    let unnamed = named.replace("  \"name\": \"@chain/contracts\",\n", "");
    assert_ne!(named, unnamed);
    age(web.parent().unwrap());
    fs::write(&path, unnamed).unwrap();
    let without = Engine::open(&web).unwrap().export().unwrap();
    age(web.parent().unwrap());
    fs::write(&path, named).unwrap();

    let refreshed = Engine::open(&web).unwrap().export().unwrap();

    let (cold, _, _) = drift_chain("fresh_shared_name_cold");
    assert!(!without.contains("\"aisle\": \"inferred\""));
    assert!(refreshed.contains("\"aisle\": \"inferred\""));
    assert_eq!(refreshed, Engine::open(&cold).unwrap().export().unwrap());
}

#[test]
fn edited_documents_relink_like_a_cold_chain() {
    let exports = |repos: [&Path; 2]| -> Vec<String> {
        repos
            .iter()
            .map(|r| Engine::open(r).unwrap().export().unwrap())
            .collect()
    };
    for (case, edit) in [
        ("fresh_documents", edit_documents as fn(&Path)),
        ("fresh_documents_listed", edit_listed_documents),
        ("fresh_documents_serve", drop_serve),
    ] {
        let (web, gateway, _) = documented_chain(case);
        age(gateway.parent().unwrap());
        let before = exports([&gateway, &web]);
        edit(&gateway);

        let refreshed = exports([&gateway, &web]);

        let (cold_web, cold, _) = documented_chain(&format!("{case}_cold"));
        edit(&cold);
        assert_ne!(refreshed[0], before[0], "{case}");
        assert_eq!(refreshed, exports([&cold, &cold_web]), "{case}");
    }
}

/// Edits that change calls, so the partition and the entry points of every repository move
/// (spec #31): the web's dashboard now calls `createOrder` (no longer an entry point), a new
/// file adds an entry point that reaches two communities, and `status.ts` is deleted; the
/// gateway loses the file of one route handler; the catalog's route stops calling into its cycle.
fn edit_calls((web, gateway, catalog): &Chain) {
    let replace = |path: PathBuf, from: &str, to: &str| {
        let src = fs::read_to_string(&path).unwrap();
        assert!(src.contains(from), "{from} in {}", path.display());
        fs::write(&path, src.replace(from, to)).unwrap();
    };
    replace(
        web.join("src/dashboard.ts"),
        "return cancelOrder(id);",
        "createOrder(id);\n  return cancelOrder(id);",
    );
    replace(
        web.join("src/dashboard.ts"),
        "import { cancelOrder, listOrders }",
        "import { cancelOrder, createOrder, listOrders }",
    );
    fs::write(
        web.join("src/audit.ts"),
        "import { cancelOrder } from \"./orders\";\nimport { health } from \"./status\";\n\n\
         export function audit(id: string) {\n  health();\n  return cancelOrder(id);\n}\n",
    )
    .unwrap();
    fs::remove_file(web.join("src/status.ts")).unwrap();
    fs::remove_file(gateway.join("src/checkout.ts")).unwrap();
    replace(
        catalog.join("app/api/products/[id]/route.ts"),
        "load(params.id, 2)",
        "params.id",
    );
}

/// The `communities` and `processes` of an export.
fn views(export: &str) -> (serde_json::Value, serde_json::Value) {
    let export: serde_json::Value = serde_json::from_str(export).unwrap();
    (export["communities"].clone(), export["processes"].clone())
}

#[test]
fn communities_and_processes_follow_edited_calls_like_a_cold_chain() {
    let exports = |(web, gateway, catalog): &Chain| -> Vec<String> {
        [web, gateway, catalog, &worker(web)]
            .iter()
            .map(|r| Engine::open(r).unwrap().export().unwrap())
            .collect()
    };
    let repos = chain("fresh_calls");
    [&repos.0, &repos.1, &repos.2].iter().for_each(|r| age(r));
    let before = exports(&repos);
    edit_calls(&repos);

    let refreshed = exports(&repos);

    let cold = chain("fresh_calls_cold");
    edit_calls(&cold);
    for (n, ((refreshed, before), cold)) in refreshed
        .iter()
        .zip(&before)
        .zip(exports(&cold))
        .enumerate()
    {
        // The worker (n = 3) is not edited: its views stay, and still equal a cold export.
        if n < 3 {
            let (communities, processes) = views(refreshed);
            let (was_communities, was_processes) = views(before);
            assert_ne!(
                communities, was_communities,
                "communities of repository {n}"
            );
            assert_ne!(processes, was_processes, "processes of repository {n}");
        }
        assert_eq!(*refreshed, cold, "repository {n}");
    }
}

/// Every repository of the chain, the worker included, exported by one engine each.
fn export_chain(web: &Path) -> Vec<String> {
    let gateway = web.with_file_name("chain-gateway");
    let catalog = web.with_file_name("chain-catalog");
    [web.to_path_buf(), gateway, catalog, worker(web)]
        .iter()
        .map(|r| Engine::open(r).unwrap().export().unwrap())
        .collect()
}

#[test]
fn the_export_does_not_depend_on_the_order_files_are_found() {
    let (web, _, _) = chain("fresh_order");
    let normal = export_chain(&web);

    let (web, _, _) = chain("fresh_order_reversed");
    let reversed = meada::with_reversed_walk(|| export_chain(&web));

    assert_eq!(reversed, normal);
}

#[test]
fn a_rust_export_does_not_depend_on_the_order_files_and_manifests_are_found() {
    let repo = fixture_repo("fresh_order_rust", "conformance-rust", ORIGIN);
    let normal = Engine::open(&repo).unwrap().export().unwrap();

    let repo = fixture_repo("fresh_order_rust_reversed", "conformance-rust", ORIGIN);
    let reversed = meada::with_reversed_walk(|| Engine::open(&repo).unwrap().export().unwrap());

    assert_eq!(reversed, normal);
}
