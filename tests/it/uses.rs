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
    let cold = edges(&repo);
    let app = repo.join("packages/app/src/App.tsx");
    let mut src = fs::read_to_string(&app).unwrap();
    src.push_str("// touched\n");
    fs::write(&app, src).unwrap();
    assert_eq!(edges(&repo), cold);
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
