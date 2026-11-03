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

    // The distribution's own name, through a re-export and through a submodule: `exact`. A
    // `requirements.txt` in `tests/` that names other packages leaves the root's in force, and
    // the one in the library's `src/shared_lib/` takes no file from its `pyproject.toml`.
    let range = "package::pypi::shared-lib >=1.0,<2 (local 1.2.0)";
    let exact = format!("{range}; import module name");
    for (caller, target) in [
        ("app/main.py::welcome::", "src/shared_lib/core.py::greet"),
        (
            "tests/test_api.py::test_greet::",
            "src/shared_lib/core.py::greet",
        ),
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
    // A top-level module only one depended-on distribution exposes: `inferred`, with the range
    // of the nearest manifest naming it (`worker/pyproject.toml`, before the root's).
    let tools = "package::pypi::shared-tools ^0.3 (local 0.3.0), range unchecked";
    assert_eq!(
        at("worker/job.py::run::"),
        [(
            "example.com/pkg/pyweb::worker/job.py::run::toolkit.polish#0",
            "example.com/pkg/pylib::tools/toolkit/__init__.py::polish",
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
fn a_relative_import_never_reaches_a_package() {
    let (_, web) = pair("pypi_relative");
    let json: Value = serde_json::from_str(&export(&web)).unwrap();

    // `from . import shared_lib`, `from .shared_lib import greet`, `from ..shared_lib.api import
    // fetch` and `from . import sl_extras` name modules of the importing package, even when no
    // such file exists and a dependency has that name.
    let site = "example.com/pkg/pyweb::app/rel.py::";
    for list in ["consumers", "links", "imports"] {
        let named =
            (json[list].as_array().into_iter().flatten()).any(|v| v.to_string().contains(site));
        assert!(!named, "{list}");
    }
}

#[test]
fn search_tells_a_pypi_package_from_an_npm_one_of_its_name() {
    let (lib, _) = pair("pypi_search");

    let found = Engine::open(&lib)
        .unwrap()
        .search("shared", Some("package"), None)
        .unwrap();

    assert_golden("pkg-pylib.search-package.txt", &found);
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

/// The `(consumer, provider, level)` of every link of `json`, sorted.
fn ends(json: &Value) -> Vec<(String, String, String)> {
    let mut found: Vec<_> = (json["links"].as_array().into_iter().flatten())
        .map(|l| {
            let at = |k: &str| l[k].as_str().unwrap().to_string();
            (at("consumer"), at("provider"), at("level"))
        })
        .collect();
    found.sort();
    found
}

#[test]
fn a_uv_source_keeps_the_one_package_it_names() {
    let origin = "https://example.com/pkg/uv-sources.git";
    let repo = fixture_repo("pypi_uv_sources", "pkg-uv-sources", Some(origin));
    let out = without_processes(&export(&repo));
    assert_golden("pkg-uv-sources.export.json", &out);
    let json: Value = serde_json::from_str(&out).unwrap();

    // `alpha` and `beta/libs/app` take the member of their own uv workspace, `pathy` the package
    // its path names; `plain`, with no uv source, fits both packages of the name.
    let id = "example.com/pkg/uv-sources::";
    let (a, b) = (
        "alpha/packages/bird-feeder/pyproject.toml",
        "beta/libs/bird-feeder/pyproject.toml",
    );
    let feed = |package: &str| {
        format!(
            "{id}{}bird_feeder/__init__.py::feed",
            package.trim_end_matches("pyproject.toml")
        )
    };
    let want = |consumer: &str, provider: &str, level: &str| {
        (
            format!("{id}{consumer}"),
            provider.to_string(),
            level.to_string(),
        )
    };
    let mut expected = Vec::new();
    for (dir, to, level) in [
        ("alpha", a, "exact"),
        ("beta/libs/app", b, "exact"),
        ("pathy", a, "exact"),
        ("plain", a, "ambiguous"),
        ("plain", b, "ambiguous"),
    ] {
        let module = dir.rsplit('/').next().unwrap();
        let entry = format!("{dir}/pyproject.toml::project.dependencies::bird-feeder");
        expected.push(want(&entry, &format!("{id}{to}"), level));
        let call = format!("{dir}/{module}/main.py::run::feed#0");
        expected.push(want(&call, &feed(to), level));
    }
    // The requirements files of `pinned` have no code, so only the entries link, to both packages.
    for file in ["requirements.txt", "requirements-dev.txt"] {
        for to in [a, b] {
            let entry = format!("pinned/{file}::requirements::bird-feeder");
            expected.push(want(&entry, &format!("{id}{to}"), "ambiguous"));
        }
    }
    // `stub` is a lone package of the consumer's own repository named like a registry one (#535):
    // with no uv source (`registry`) it is only `inferred`, with its path named (`local`) `exact`.
    let (stub, call) = ("stub/pyproject.toml", "stub/anyio/__init__.py::sleep");
    for (dir, level) in [("registry", "inferred"), ("local", "exact")] {
        let entry = format!("{dir}/pyproject.toml::project.dependencies::anyio");
        expected.push(want(&entry, &format!("{id}{stub}"), level));
        let site = format!("{dir}/{dir}/main.py::run::sleep#0");
        expected.push(want(&site, &format!("{id}{call}"), level));
    }
    // A `requirements.txt` names no source either.
    let entry = "registry/requirements.txt::requirements::anyio";
    expected.push(want(entry, &format!("{id}{stub}"), "inferred"));
    expected.sort();
    assert_eq!(ends(&json), expected);
    let reason = |consumer: &str| {
        let consumer = format!("{id}{consumer}/pyproject.toml::project.dependencies::anyio");
        let mut link = json["links"].as_array().unwrap().iter();
        let link = link.find(|l| l["consumer"] == consumer.as_str()).unwrap();
        link["reason"].as_str().unwrap().to_string()
    };
    assert_eq!(
        reason("registry"),
        "no uv source names the local package, the registry package is the likely provider; \
         package::pypi::anyio >=4,<5 (local 4.3.0+foo)"
    );
    assert_eq!(
        reason("local"),
        "package::pypi::anyio >=4,<5 (local 4.3.0+foo)"
    );

    // A pin no local version satisfies says `range mismatch`; a range one satisfies adds no
    // words, and one rejects the other version (#534). Neither drops a candidate.
    let mut reasons: Vec<(String, String)> = (json["links"].as_array().unwrap().iter())
        .filter_map(|l| {
            let consumer = l["consumer"]
                .as_str()?
                .strip_prefix(id)?
                .strip_prefix("pinned/")?;
            let provider = l["provider"].as_str()?.strip_prefix(id)?;
            let reason = l["reason"]
                .as_str()?
                .strip_prefix("2 providers fit; package::pypi::")?;
            Some((format!("{consumer} {provider}"), reason.to_string()))
        })
        .collect();
    reasons.sort();
    let pin = "requirements.txt::requirements::bird-feeder";
    let dev = "requirements-dev.txt::requirements::bird-feeder";
    let said = |consumer: &str, provider: &str, reason: &str| {
        (
            format!("{consumer} {provider}"),
            format!("bird-feeder {reason}"),
        )
    };
    let mut want = vec![
        said(pin, a, "==24.4.2 (local 1.0.0), range mismatch"),
        said(pin, b, "==24.4.2 (local 2.0.0), range mismatch"),
        said(dev, a, ">=1,<2 (local 1.0.0)"),
        said(dev, b, ">=1,<2 (local 2.0.0), range mismatch"),
    ];
    want.sort();
    assert_eq!(reasons, want);
}

#[test]
fn a_uv_git_source_keeps_the_repository_of_its_url_and_a_url_source_stays_out() {
    let case = "pypi_uv_git";
    fixture_repo(
        case,
        "pkg-uv-feeder",
        Some("https://example.com/pkg/uv-feeder.git"),
    );
    let origin = "https://example.com/pkg/uv-sources.git";
    let repo = fixture_repo(case, "pkg-uv-sources", Some(origin));
    fs::write(
        repo.join("meada.toml"),
        "[[peer]]\nrepo = \"example.com/pkg/uv-feeder\"\n",
    )
    .unwrap();
    let pathy = repo.join("pathy/pyproject.toml");
    let source = |line: &str| {
        let text = fs::read_to_string(&pathy).unwrap();
        let (head, _) = text.split_once("bird-feeder = {").unwrap();
        fs::write(&pathy, format!("{head}{line}\n")).unwrap();
    };
    let of = |json: &Value, consumer: &str| -> Vec<(String, String)> {
        (ends(json).into_iter())
            .filter(|(c, ..)| c.ends_with(consumer))
            .map(|(_, p, level)| (p, level))
            .collect()
    };
    let entry = "pathy/pyproject.toml::project.dependencies::bird-feeder";

    source("bird-feeder = { git = \"https://example.com/pkg/uv-feeder.git\", tag = \"v3\" }");
    let json: Value = serde_json::from_str(&export(&repo)).unwrap();
    let feeder = "example.com/pkg/uv-feeder::pyproject.toml".to_string();
    assert_eq!(of(&json, entry), [(feeder.clone(), "exact".to_string())]);
    // `plain`, with no source, fits the peer's package too.
    let plain = of(
        &json,
        "plain/pyproject.toml::project.dependencies::bird-feeder",
    );
    assert_eq!(plain.len(), 3);
    assert!(plain.contains(&(feeder, "ambiguous".to_string())));

    for outside in [
        "bird-feeder = { git = \"https://example.com/pkg/elsewhere.git\" }",
        "bird-feeder = { url = \"https://example.com/bird_feeder-3.0.0.whl\" }",
        "bird-feeder = { index = \"internal\" }",
    ] {
        source(outside);
        let json: Value = serde_json::from_str(&export(&repo)).unwrap();
        assert_eq!(of(&json, entry), [], "{outside}");
        assert_eq!(
            of(&json, "pathy/pathy/main.py::run::feed#0"),
            [],
            "{outside}"
        );
    }
}
