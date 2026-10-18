//! PyPI packages (spec #24): a named `pyproject.toml` of a workspace repository provides
//! `package::pypi::<PEP 503 name>`, the dependencies of `pyproject.toml` and `requirements*.txt`
//! consume it, and an import of one of its modules crosses into its symbols.

use crate::common;

use std::fs;
use std::path::{Path, PathBuf};

use common::{age, assert_golden, fixture_repo, without_processes};
use meada::engine::{Direction, Engine, Start};
use serde_json::Value;

/// The library and its consumer as siblings: `pyweb` names `pylib` as its peer.
fn pair(case: &str) -> (PathBuf, PathBuf) {
    let repo = |name: &str| {
        let origin = format!("https://example.com/pkg/{name}.git");
        fixture_repo(case, &format!("pkg-{name}"), Some(&origin))
    };
    (repo("pylib"), repo("pyweb"))
}

fn export(repo: &Path) -> String {
    Engine::open(repo).unwrap().export().unwrap()
}

const WELCOME_GREET: &str = "example.com/pkg/pyweb::app/main.py::welcome::greet#0";
const GREET: &str = "example.com/pkg/pylib::src/shared_lib/core.py::greet";

#[test]
fn python_dependencies_and_imports_link_to_the_distribution() {
    let (lib, web) = pair("pypi_links");

    assert_golden("pkg-pyweb.export.json", &without_processes(&export(&web)));
    assert_golden("pkg-pylib.export.json", &without_processes(&export(&lib)));
}

/// The `(consumer, provider, level, reason)` of every link of `json` whose consumer starts
/// with `site`.
fn links<'j>(json: &'j Value, site: &str) -> Vec<(&'j str, &'j str, &'j str, &'j str)> {
    (json["links"].as_array().into_iter().flatten())
        .filter(|l| l["consumer"].as_str().unwrap().starts_with(site))
        .map(|l| {
            let at = |k: &str| l[k].as_str().unwrap();
            (at("consumer"), at("provider"), at("level"), at("reason"))
        })
        .collect()
}

#[test]
fn each_import_level_reads_from_the_module_it_names() {
    let (_, web) = pair("pypi_levels");
    let json: Value = serde_json::from_str(&export(&web)).unwrap();
    let at = |caller: &str| links(&json, &format!("example.com/pkg/pyweb::{caller}"));
    let lib = "example.com/pkg/pylib::";

    // The distribution's own name, through a re-export and through a submodule: `exact`.
    let range = "package::pypi::shared-lib >=1.0,<2 (local 1.2.0), range unchecked";
    let exact = format!("{range}; import module name");
    for (caller, target) in [
        ("app/main.py::welcome::", "src/shared_lib/core.py::greet"),
        ("app/main.py::lookup::", "src/shared_lib/api.py::fetch"),
        (
            "app/main.py::make::",
            "src/shared_lib/api.py::Client.create",
        ),
    ] {
        let target = format!("{lib}{target}");
        let found: Vec<_> = at(caller)
            .into_iter()
            .map(|(_, p, l, r)| (p, l, r))
            .collect();
        assert_eq!(
            found,
            [(target.as_str(), "exact", exact.as_str())],
            "{caller}"
        );
    }
    // A top-level module only one depended-on distribution exposes: `inferred`.
    let tools = "package::pypi::shared-tools ^0.3 (local 0.3.0), range unchecked";
    assert_eq!(
        at("worker/job.py::run::"),
        [(
            "example.com/pkg/pyweb::worker/job.py::run::sl_extras.boost#0",
            "example.com/pkg/pylib::tools/sl_extras/__init__.py::boost",
            "inferred",
            format!("{tools}; import top-level module").as_str(),
        )]
    );
    // The same module exposed by both distributions the root manifests depend on: `ambiguous`.
    let extra = at("app/main.py::extra::");
    let reached: Vec<(&str, &str)> = extra.iter().map(|(_, p, l, _)| (*p, *l)).collect();
    assert_eq!(
        reached,
        [
            (
                "example.com/pkg/pylib::src/sl_extras/__init__.py::boost",
                "ambiguous"
            ),
            (
                "example.com/pkg/pylib::tools/sl_extras/__init__.py::boost",
                "ambiguous"
            ),
        ]
    );
    // A local binding of the imported name, and a module outside the workspace, do not cross.
    assert!(at("app/main.py::hidden::").is_empty());
    assert!(at("app/main.py::encode::").is_empty());
}

#[test]
fn an_npm_and_a_pypi_package_of_one_name_never_link() {
    let (_, web) = pair("pypi_npm_names");
    let json: Value = serde_json::from_str(&export(&web)).unwrap();

    let all = links(&json, "example.com/pkg/pyweb::");
    // `pylib` holds an npm `shared-lib` too, and `ui/package.json` names `shared-tools` (a PyPI
    // distribution only): neither crosses ecosystems.
    assert!(!all.is_empty());
    for (consumer, provider, _, reason) in all {
        assert!(
            !provider.ends_with("js/package.json"),
            "{consumer} -> {provider}"
        );
        assert!(!consumer.contains("ui/package.json"), "{consumer}");
        assert!(reason.starts_with("package::pypi::"), "{reason}");
    }
}

#[test]
fn context_on_a_library_function_lists_its_python_consumers() {
    let (lib, _) = pair("pypi_context");

    let found = Engine::open(&lib).unwrap().context(GREET, None).unwrap();

    assert_golden("pkg-pylib.context-greet.txt", &found);
}

#[test]
fn trace_down_crosses_the_package_hop_and_onward() {
    let (_, web) = pair("pypi_trace_down");
    let engine = Engine::open(&web).unwrap();

    let down = engine
        .trace(Start::From(WELCOME_GREET), Direction::Down, None)
        .unwrap();

    assert_golden("pkg-pyweb.trace-down.txt", &down);
}

/// Bumps the library, narrows the `pyproject.toml` range, drops `shared-tools` from the root
/// `requirements.txt` (so `sl_extras` from the root resolves through `shared-lib` alone) and
/// adds a requirements file that names the library in `worker/`.
fn edit(lib: &Path, web: &Path) {
    let replace = |repo: &Path, file: &str, from: &str, to: &str| {
        let text = fs::read_to_string(repo.join(file)).unwrap();
        assert!(text.contains(from), "{file} lacks {from}");
        fs::write(repo.join(file), text.replace(from, to)).unwrap();
    };
    replace(lib, "pyproject.toml", "1.2.0", "1.3.0");
    replace(web, "pyproject.toml", ">=1.0,<2", "~=1.3");
    replace(
        web,
        "requirements.txt",
        "Shared.Tools==0.3.0",
        "# Shared.Tools==0.3.0",
    );
    fs::write(web.join("worker/requirements.txt"), "shared_lib\n").unwrap();
}

#[test]
fn an_edited_pyproject_or_requirements_file_relinks_like_a_cold_export() {
    let (lib, web) = pair("pypi_fresh");
    age(lib.parent().unwrap());
    let both = |lib: &Path, web: &Path| [export(web), export(lib)];
    let before = both(&lib, &web);
    edit(&lib, &web);

    let refreshed = both(&lib, &web);

    let (cold_lib, cold_web) = pair("pypi_fresh_cold");
    edit(&cold_lib, &cold_web);
    assert_ne!(refreshed, before);
    assert_eq!(refreshed, both(&cold_lib, &cold_web));
    let json: Value = serde_json::from_str(&refreshed[0]).unwrap();
    let extra = links(&json, "example.com/pkg/pyweb::app/main.py::extra::");
    let levels: Vec<&str> = extra.iter().map(|(_, _, l, _)| *l).collect();
    assert_eq!(levels, ["inferred"]);
}
