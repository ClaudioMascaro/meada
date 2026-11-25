//! Non-call uses of names (TS/JS): a name rendered as JSX, written as a type, read as a value or
//! a member, extended, re-exported or taken off a dynamic `import()` is a `calls` row of its
//! kind, from the symbol that holds it to the symbol it names, `exact` only.

use crate::common;

use std::fs;
use std::path::{Path, PathBuf};

use common::fixture_repo;
use meada::engine::{Direction, Engine, Start};

const IDENTITY: &str = "example.com/pkg/uses";

fn uses_repo(case: &str) -> PathBuf {
    fixture_repo(case, "pkg-uses", Some("https://example.com/pkg/uses.git"))
}

fn export(repo: &Path) -> serde_json::Value {
    serde_json::from_str(&Engine::open(repo).unwrap().export().unwrap()).unwrap()
}

fn cut(id: &str) -> String {
    id.strip_prefix(IDENTITY)
        .and_then(|s| s.strip_prefix("::"))
        .unwrap_or(id)
        .to_string()
}

fn text(v: &serde_json::Value) -> String {
    v.as_str().map(cut).unwrap_or_default()
}

/// The export's `calls` rows with a kind, as (caller, kind, target, level), sorted.
fn edges(repo: &Path) -> Vec<(String, String, String, String)> {
    let json = export(repo);
    let mut rows: Vec<_> = (json["calls"].as_array().into_iter().flatten())
        .filter(|c| c.get("kind").is_some())
        .map(|c| {
            (
                text(&c["caller"]),
                text(&c["kind"]),
                text(&c["target"]),
                text(&c["level"]),
            )
        })
        .collect();
    rows.sort();
    rows
}

/// (row id, holder) of every `calls` row with a holder, sorted.
fn holders(repo: &Path) -> Vec<(String, String)> {
    let json = export(repo);
    let mut rows: Vec<_> = (json["calls"].as_array().into_iter().flatten())
        .filter(|c| c.get("holder").is_some())
        .map(|c| (text(&c["id"]), text(&c["holder"])))
        .collect();
    rows.sort();
    rows
}

fn edge(caller: &str, kind: &str, target: &str) -> (String, String, String, String) {
    let app = |s: &str| format!("packages/app/src/{s}");
    (app(caller), kind.into(), app(target), "exact".into())
}

#[test]
fn a_use_in_the_same_file_or_package_is_an_exact_edge_of_its_kind() {
    let repo = uses_repo("uses_edges");
    let got = edges(&repo);
    for want in [
        edge("LoginPage.tsx::LoginPage", "jsx", "LoginPage.tsx::Header"),
        edge(
            "LoginPage.tsx::LoginPage",
            "value",
            "LoginPage.tsx::handler",
        ),
        edge("App.tsx::App", "value", "LoginPage.tsx::routes"),
        edge("load.ts::load", "member", "client.ts::client"),
        edge("backend.ts::wire", "value", "plugin.ts::plugin"),
        edge("LoginPage.tsx::LoginPage", "type", "kinds.ts::Kind"),
        edge("LoginPage.tsx::LoginPage", "member", "kinds.ts::Kind"),
        edge("dyn.ts::four", "dynamic", "client.ts::client"),
        // An enum used in its own file: no reading binds it, the file's declarations do.
        edge("LoginPage.tsx::useLocal", "member", "LoginPage.tsx::Local"),
    ] {
        assert!(got.contains(&want), "{want:?} missing from {got:#?}");
    }
}

#[test]
fn a_use_never_links_by_name() {
    let repo = uses_repo("uses_by_name");
    let got = edges(&repo);
    assert!(!got.is_empty());
    for (caller, kind, target, level) in &got {
        assert!(
            level == "exact" || (level.is_empty() && target.is_empty()),
            "{caller} {kind} {target} {level}"
        );
        let name = target.rsplit("::").next().unwrap();
        assert!(
            !["div", "t", "m", "Promise", "Record", "console"].contains(&name),
            "{caller} {kind} {target}"
        );
        // A parameter, a local and a global named like another file's functions.
        assert!(!caller.contains("/neg.tsx::"), "{caller} {kind} {target}");
    }
}

#[test]
fn a_row_held_by_a_top_level_declaration_names_its_holder() {
    let repo = uses_repo("uses_holders");
    let got = holders(&repo);
    let want = |id: &str, holder: &str| {
        (
            format!("packages/app/src/{id}"),
            format!("packages/app/src/{holder}"),
        )
    };
    for want in [
        want("LoginPage.tsx::(jsx)LoginPage#0", "LoginPage.tsx::routes"),
        want("plugin.ts::helper#0", "plugin.ts::plugin"),
        want("LoginPage.tsx::(member)tokens.gap#0", "LoginPage.tsx::gap"),
    ] {
        assert!(got.contains(&want), "{want:?} missing from {got:#?}");
    }
    // A row whose caller is a symbol has no holder.
    assert!(got.iter().all(|(id, _)| !id.contains("::LoginPage::")));
}

#[test]
fn a_call_and_a_use_of_one_name_keep_apart() {
    let repo = uses_repo("uses_apart");
    let json = export(&repo);
    let row = |id: &str| {
        let id = format!("{IDENTITY}::packages/app/src/LoginPage.tsx::both::{id}");
        (json["calls"].as_array().unwrap().iter())
            .find(|c| c["id"] == id.as_str())
            .cloned()
            .unwrap_or_else(|| panic!("no row {id}"))
    };
    assert!(row("Button#0").get("kind").is_none());
    assert_eq!(row("(jsx)Button#0")["kind"], "jsx");
}

#[test]
fn a_use_of_a_value_and_type_name_links_once() {
    let repo = uses_repo("uses_value_and_type");
    let got: Vec<_> = (edges(&repo).into_iter())
        .filter(|(caller, ..)| caller.ends_with("::useFoo"))
        .map(|(_, kind, target, _)| (kind, target))
        .collect();
    let foo = "packages/app/src/LoginPage.tsx::Foo".to_string();
    assert_eq!(
        got,
        [
            ("type".to_string(), foo.clone()),
            ("value".to_string(), foo)
        ]
    );
}

#[test]
fn a_warm_index_keeps_the_use_rows() {
    let repo = uses_repo("uses_warm");
    let cold = (edges(&repo), export(&repo));
    let app = repo.join("packages/app/src/App.tsx");
    let mut src = fs::read_to_string(&app).unwrap();
    src.push_str("// touched\n");
    fs::write(&app, src).unwrap();
    let warm = (edges(&repo), export(&repo));
    assert_eq!(warm.0, cold.0);
    // The doors the stored rows keep open, and the package rows (the second hop included).
    for section in ["processes", "imports"] {
        assert_eq!(warm.1[section], cold.1[section], "{section}");
    }
}

#[test]
fn doors_and_calls_stay_invocation_only() {
    let repo = uses_repo("uses_doors");
    let json = export(&repo);
    let id = format!("{IDENTITY}::packages/app/src/LoginPage.tsx::LoginPage");
    let doors: Vec<_> = (json["processes"].as_array().into_iter().flatten())
        .filter(|p| p["kind"] == "export")
        .map(|p| p["entry_point"].as_str().unwrap_or_default().to_string())
        .collect();
    assert!(doors.contains(&id), "{id} not a door in {doors:#?}");
}

fn app(id: &str) -> String {
    format!("{IDENTITY}::packages/app/src/{id}")
}

#[test]
fn trace_up_walks_uses_and_holders() {
    let repo = uses_repo("uses_trace");
    let engine = Engine::open(&repo).unwrap();
    let up = |id: &str| {
        engine
            .trace(Start::From(&app(id)), Direction::Up, None)
            .unwrap()
    };
    // Header is rendered by LoginPage, which `routes` renders, which App reads.
    let from_header = up("LoginPage.tsx::Header");
    for name in ["LoginPage (jsx)", "routes (jsx)", "App (value)"] {
        assert!(
            from_header.contains(name),
            "{name} missing from:\n{from_header}"
        );
    }
    // helper is called inside `plugin`'s initializer, which `wire` reads.
    let from_helper = up("plugin.ts::helper");
    for name in [" plugin", "wire (value)"] {
        assert!(
            from_helper.contains(name),
            "{name} missing from:\n{from_helper}"
        );
    }
}

#[test]
fn context_lists_uses_apart_from_calls() {
    let repo = uses_repo("uses_context");
    let engine = Engine::open(&repo).unwrap();
    let page = engine
        .context(&app("LoginPage.tsx::LoginPage"), None)
        .unwrap();
    let section = |text: &str, name: &str| -> Vec<String> {
        let lines: Vec<&str> = text.lines().collect();
        let Some(at) = lines.iter().position(|l| *l == format!("{name}:")) else {
            return Vec::new();
        };
        (lines[at + 1..].iter())
            .take_while(|l| !l.ends_with(':') && !l.ends_with(": none"))
            .map(|l| l.to_string())
            .collect()
    };
    let uses = section(&page, "uses");
    for want in ["Header (jsx)", "handler (value)"] {
        assert!(
            uses.iter().any(|l| l.ends_with(want)),
            "{want} not in uses:\n{page}"
        );
    }
    assert!(
        !section(&page, "callees")
            .iter()
            .any(|l| l.contains("Header")),
        "{page}"
    );
    let header = engine.context(&app("LoginPage.tsx::Header"), None).unwrap();
    let used_by = section(&header, "used by");
    assert!(
        used_by.iter().any(|l| l.ends_with("LoginPage (jsx)")),
        "{header}"
    );
    assert!(header.contains("callers: none"), "{header}");
    // A top-level declaration lists the uses its initializer makes, as `used by:` names it.
    let routes = engine.context(&app("LoginPage.tsx::routes"), None).unwrap();
    assert!(
        section(&routes, "uses")
            .iter()
            .any(|l| l.ends_with("LoginPage (jsx)")),
        "{routes}"
    );
}

#[test]
fn processes_are_unchanged_by_uses() {
    let repo = uses_repo("uses_processes");
    let json = export(&repo);
    let calls: std::collections::HashSet<(String, String)> = (json["calls"].as_array().unwrap())
        .iter()
        .filter(|c| c.get("kind").is_none())
        .filter_map(|c| {
            Some((
                c["caller"].as_str()?.to_string(),
                c["target"].as_str()?.to_string(),
            ))
        })
        .collect();
    let mut stepped = 0;
    for p in json["processes"].as_array().unwrap() {
        let mut path: Vec<String> = Vec::new();
        for step in p["steps"].as_array().unwrap() {
            let depth = step["depth"].as_u64().unwrap() as usize;
            let node = step["node"].as_str().unwrap().to_string();
            path.truncate(depth);
            if let Some(parent) = path.last()
                && step["via"] == "call"
            {
                stepped += 1;
                assert!(
                    calls.contains(&(parent.clone(), node.clone())),
                    "{parent} -> {node} is no call"
                );
            }
            path.push(node);
        }
    }
    assert!(stepped > 0);
}

/// The export's `imports` rows at `file` that name `specifier`, whole and without their `site`,
/// sorted by their JSON text (front 1's `rows_at`, `tests/it/packages.rs`).
fn rows_at(repo: &Path, file: &str, specifier: &str) -> Vec<serde_json::Value> {
    let json = export(repo);
    let mut rows: Vec<serde_json::Value> = (json["imports"].as_array().into_iter().flatten())
        .filter(|i| i["site"].as_str().and_then(|s| s.split("::").nth(1)) == Some(file))
        .filter(|i| i["specifier"] == specifier)
        .cloned()
        .map(|mut i| {
            i.as_object_mut().unwrap().remove("site");
            i
        })
        .collect();
    rows.sort_by_key(|r| r.to_string());
    rows
}

/// (level, name, target) of each row of `rows`, the target cut, sorted and deduplicated.
fn named(rows: &[serde_json::Value]) -> Vec<(String, String, String)> {
    let mut found: Vec<_> = (rows.iter())
        .map(|r| (text(&r["level"]), text(&r["name"]), text(&r["target"])))
        .collect();
    found.sort();
    found.dedup();
    found
}

fn exact(name: &str, target: &str) -> (String, String, String) {
    ("exact".into(), name.into(), format!("packages/{target}"))
}

/// The export entry points of the export, cut.
fn doors(json: &serde_json::Value) -> Vec<String> {
    (json["processes"].as_array().unwrap().iter())
        .filter(|p| p["kind"] == "export")
        .map(|p| text(&p["entry_point"]))
        .collect()
}

#[test]
fn a_use_across_packages_gives_an_imports_row() {
    let repo = uses_repo("uses_packages");
    let rows = rows_at(&repo, "packages/app/src/LoginPage.tsx", "@scope/ui");
    assert_eq!(
        named(&rows),
        [
            exact("Base", "ui/src/index.ts::Base"),
            exact("Button", "ui/src/Button.tsx::Button"),
            exact("Mode", "ui/src/theme.ts::Mode"),
            exact("Theme", "ui/src/theme.ts::Theme"),
            exact("tokens", "ui/src/index.ts::tokens"),
        ]
    );
}

#[test]
fn dynamic_imports_give_rows_for_the_member_they_take() {
    let repo = uses_repo("uses_dynamic");
    let lazy = rows_at(&repo, "packages/app/src/lazy.tsx", "@scope/a");
    assert_eq!(named(&lazy), [exact("Page", "a/src/index.ts::Page")]);
    let json = export(&repo);
    let at = |caller: &str| -> Vec<String> {
        let prefix = format!("{IDENTITY}::packages/app/src/dyn.ts::{caller}::");
        (json["imports"].as_array().unwrap().iter())
            .filter(|i| i["site"].as_str().unwrap().starts_with(&prefix))
            .map(|i| format!("{} {}", text(&i["name"]), text(&i["target"])))
            .collect()
    };
    for caller in ["one", "two"] {
        assert_eq!(at(caller), ["X packages/a/src/index.ts::X"], "{caller}");
    }
    assert!(at("three").is_empty());
}

#[test]
fn start_test_backend_features_import_their_default() {
    let repo = uses_repo("uses_test_backend");
    let json = export(&repo);
    let at = |caller: &str, specifier: &str| -> Vec<(String, String, String)> {
        let prefix = format!("{IDENTITY}::packages/app/src/test-backend.ts::{caller}::");
        let rows: Vec<serde_json::Value> = (json["imports"].as_array().unwrap().iter())
            .filter(|i| i["site"].as_str().unwrap().starts_with(&prefix))
            .filter(|i| i["specifier"] == specifier)
            .cloned()
            .collect();
        named(&rows)
    };
    assert_eq!(
        at("run", "@scope/a"),
        [exact("default", "a/src/index.ts::feature")]
    );
    // `@scope/b` has no default export: what a static `import d from '@scope/b'` gives.
    assert!(
        at("run", "@scope/b")
            .iter()
            .all(|(level, name, _)| name == "default" && level != "exact")
    );
    assert!(at("no", "@scope/a").is_empty());
}

#[test]
fn a_default_a_test_backend_adds_is_no_export_door() {
    let repo = uses_repo("uses_test_backend_door");
    assert!(!doors(&export(&repo)).contains(&"packages/a/src/index.ts::feature".to_string()));
    // Without the test backend nothing imports it: the drop above is what keeps it out.
    let bare = uses_repo("uses_test_backend_no_door");
    fs::remove_file(bare.join("packages/app/src/test-backend.ts")).unwrap();
    assert!(doors(&export(&bare)).contains(&"packages/a/src/index.ts::feature".to_string()));
}

#[test]
fn an_import_row_from_a_use_drops_no_door() {
    let repo = uses_repo("uses_doors_kept");
    let json = export(&repo);
    // The row exists: the door stays although an imports row targets it. (`Button` is no door,
    // `both` calls it; `Base` is a class, never one.)
    let card = rows_at(&repo, "packages/app/src/card.tsx", "@scope/ui");
    assert_eq!(named(&card), [exact("Card", "ui/src/index.ts::Card")]);
    let doors = doors(&json);
    let card = "packages/ui/src/index.ts::Card".to_string();
    assert!(doors.contains(&card), "{card} not a door: {doors:?}");
}

#[test]
fn a_name_reexported_from_another_package_links_to_the_original() {
    let repo = uses_repo("uses_second_hop");
    for specifier in ["@scope/b", "@scope/s"] {
        let rows = rows_at(&repo, "packages/app/src/xb.ts", specifier);
        assert_eq!(
            named(&rows),
            [exact("X", "a/src/index.ts::X")],
            "{specifier}"
        );
    }
}

#[test]
fn a_paths_alias_a_file_answers_is_no_package() {
    let repo = uses_repo("uses_alias");
    let found = edges(&repo);
    for (caller, kind) in [
        ("packages/app/src/alias.ts::five", "dynamic"),
        ("packages/app/src/alias.ts", "reexport"),
    ] {
        let want = (
            caller.to_string(),
            kind.to_string(),
            "packages/app/src/client.ts::client".to_string(),
            "exact".to_string(),
        );
        assert!(found.contains(&want), "{want:?} not in {found:#?}");
    }
}

#[test]
fn one_name_taken_off_two_imports_in_one_scope_keeps_two_rows() {
    let repo = uses_repo("uses_ordinal");
    let json = export(&repo);
    let prefix = format!("{IDENTITY}::packages/app/src/dyn.ts::six::(dynamic)X#");
    let mut sites: Vec<(String, String)> = (json["imports"].as_array().unwrap().iter())
        .filter(|i| i["site"].as_str().unwrap().starts_with(&prefix))
        .map(|i| (text(&i["site"]), text(&i["specifier"])))
        .collect();
    sites.sort();
    let at = |n: usize, s: &str| {
        (
            format!("packages/app/src/dyn.ts::six::(dynamic)X#{n}"),
            s.to_string(),
        )
    };
    assert_eq!(sites, [at(0, "@scope/a"), at(1, "@scope/b")]);
}

/// `tests/fixtures/go-type-uses`, as `example.com/go/types`.
fn go_types(case: &str) -> PathBuf {
    fixture_repo(
        case,
        "go-type-uses",
        Some("https://example.com/go/types.git"),
    )
}

/// The export's `calls` rows of kind `type`, as (caller, target, level, holder), sorted, with
/// the identity cut off.
fn go_type_rows(repo: &Path) -> Vec<(String, String, String, String)> {
    let json = export(repo);
    let cut = |v: &serde_json::Value| {
        let s = v.as_str().unwrap_or_default();
        s.strip_prefix("example.com/go/types::")
            .unwrap_or(s)
            .to_string()
    };
    let mut rows: Vec<_> = (json["calls"].as_array().into_iter().flatten())
        .filter(|c| c["kind"] == "type")
        .map(|c| {
            (
                cut(&c["caller"]),
                cut(&c["target"]),
                cut(&c["level"]),
                cut(&c["holder"]),
            )
        })
        .collect();
    rows.sort();
    rows
}

#[test]
fn a_go_handler_uses_the_request_struct_it_declares() {
    let rows = go_type_rows(&go_types("uses_go_handler"));
    let row = (
        "api/handler.go::Handler".to_string(),
        "svc/types.go::Req".to_string(),
        "exact".to_string(),
        String::new(),
    );
    assert!(rows.contains(&row), "{rows:#?}");
}

#[test]
fn a_struct_field_type_is_held_by_its_struct() {
    let rows = go_type_rows(&go_types("uses_go_field"));
    let row = (
        "svc/types.go".to_string(),
        "svc/types.go::Inner".to_string(),
        "exact".to_string(),
        "svc/types.go::Outer".to_string(),
    );
    assert!(rows.contains(&row), "{rows:#?}");
    let second = (
        row.0.clone(),
        row.1.clone(),
        row.2.clone(),
        "svc/types.go::Second".into(),
    );
    assert!(rows.contains(&second), "{rows:#?}");
}

#[test]
fn an_external_go_type_gives_no_row() {
    let json = export(&go_types("uses_go_external"));
    let external = (json["calls"].as_array().into_iter().flatten())
        .filter(|c| c["kind"] == "type")
        .filter(|c| {
            c["target"]
                .as_str()
                .is_none_or(|t| !t.starts_with("example.com/go/types::"))
        })
        .count();
    assert_eq!(external, 0, "{:#?}", json["calls"]);
}

#[test]
fn a_go_type_change_affects_its_users() {
    let repo = go_types("uses_go_affected");
    let file = repo.join("svc/types.go");
    let text = fs::read_to_string(&file).unwrap();
    fs::write(
        &file,
        text.replace("\tName string\n", "\tName  string `json:\"name\"`\n"),
    )
    .unwrap();
    common::git(&repo, &["add", "-A"]);
    common::git(&repo, &["commit", "-q", "-m", "edit"]);
    let changes = meada::engine::Changes::Range {
        base: "HEAD~1",
        head: "HEAD",
    };
    let answer = Engine::open(&repo).unwrap().affected(changes).unwrap();
    let answer = serde_json::to_value(answer).unwrap();
    let handler =
        (answer["files"].as_array().into_iter().flatten()).find(|f| f["path"] == "api/handler.go");
    let handler = handler.unwrap_or_else(|| panic!("{answer:#}"));
    assert_eq!(handler["level"], "exact", "{handler}");
}
