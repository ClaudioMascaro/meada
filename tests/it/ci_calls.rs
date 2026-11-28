use std::fs;
use std::path::{Path, PathBuf};

use meada::engine::{Direction, Engine, Start};
use serde_json::Value;

use crate::common::{assert_golden, fixture_repo, git, meada};

const PUBLISH: &str = ".github/workflows/publish.yml";
const CI: &str = ".github/workflows/ci.yml";
const CONTRACT: &str = "ci::github.com/acme/workflows/.github/workflows/publish.yml";

const SIDE_WORDS: [&str; 4] = ["provides ", "consumes ", "  consumer: ", "  provider: "];

pub(crate) fn reproduction(case: &str) -> [PathBuf; 5] {
    ["workflows", "wrapper", "app-a", "app-b", "app-c"].map(|name| {
        let origin = format!("https://github.com/acme/{name}.git");
        fixture_repo(case, &format!("ci-{name}"), Some(&origin))
    })
}

pub(crate) fn indexed(root: &Path) -> String {
    git(root, &["rev-parse", "HEAD"])[..7].to_string()
}

pub(crate) fn exported(engine: &Engine) -> Value {
    serde_json::from_str(&engine.export().unwrap()).unwrap()
}

fn column<'a>(export: &'a Value, section: &str, key: &str) -> Vec<&'a str> {
    (export[section].as_array().unwrap().iter())
        .map(|row| row[key].as_str().unwrap())
        .collect()
}

fn pipelines(contracts: Vec<&str>) -> Vec<&str> {
    let pipeline = |contract: &&str| contract.contains("/.github/workflows/");
    contracts.into_iter().filter(pipeline).collect()
}

fn sided(answer: &str) -> bool {
    (answer.lines()).any(|line| SIDE_WORDS.iter().any(|word| line.starts_with(word)))
}

pub(crate) fn listed<'a>(answer: &'a str, prefix: &str) -> Vec<&'a str> {
    (answer.lines())
        .filter_map(|line| line.strip_prefix(prefix))
        .collect()
}

#[test]
fn a_reusable_pipeline_lists_every_ci_job_of_the_workspace_that_calls_it() {
    let [workflows, ..] = reproduction("ci_calls_consumers");
    let engine = Engine::open(&workflows).unwrap();
    let answer = engine.context(PUBLISH, None).unwrap();
    let at = indexed(&workflows);
    let ci = ".github/workflows/ci.yml";
    assert_eq!(
        listed(&answer, "  consumer: "),
        [
            format!(
                "github.com/acme/app-a {ci}:12 jobs.publish [exact: @main (indexed {at})] with: package, registry; secrets: inherit; permissions: contents: read, id-token: write"
            ),
            format!(
                "github.com/acme/app-b {ci}:11 jobs[\"publish-prd\"] [exact: @v1 (indexed {at}), permissions mismatch: id-token write against none] with: package; secrets: token; permissions: contents: read"
            ),
            format!(
                "github.com/acme/app-b {ci}:18 jobs[\"publish-stg\"] [exact: @main (indexed {at})] with: package; permissions: contents: read, id-token: write"
            ),
            format!(
                "github.com/acme/wrapper .github/workflows/release.yml:5 jobs.release [exact: @main (indexed {at})] with: package; secrets: inherit; permissions: contents: read, id-token: write"
            ),
        ],
        "{answer}"
    );
    assert_eq!(
        listed(&answer, "provides "),
        ["ci::github.com/acme/workflows/.github/workflows/publish.yml"],
        "{answer}"
    );
}

#[test]
fn a_pipeline_provides_only_with_workflow_call_and_its_identifier_is_lower_cased() {
    let [workflows, _, app_a, _, app_c] = reproduction("ci_calls_providers");
    fs::write(
        app_c.join(".github/workflows/case.yml"),
        "on: push\njobs:\n  cased:\n    uses: acme/workflows/.github/workflows/Publish.yml@main\n",
    )
    .unwrap();
    let engine = Engine::open(&workflows).unwrap();
    let export = exported(&engine);
    let file = |name: &str| format!("ci::github.com/acme/workflows/.github/workflows/{name}");
    assert_eq!(
        pipelines(column(&export, "providers", "contract")),
        [
            file("forms.yml"),
            file("lint.yml"),
            file("publish.yml"),
            file("scan.yml")
        ]
    );
    let internal = engine
        .context(".github/workflows/internal.yml", None)
        .unwrap();
    assert!(!sided(&internal), "{internal}");
    let scan = engine.context(".github/workflows/scan.yml", None).unwrap();
    let at = indexed(&workflows);
    assert_eq!(listed(&scan, "provides "), [file("scan.yml")], "{scan}");
    assert_eq!(
        listed(&scan, "  consumer: "),
        [
            format!("github.com/acme/app-a {CI}:24 jobs.scan [exact: @v2 (indexed {at})] permissions: not declared"),
            "github.com/acme/workflows .github/workflows/forms.yml:8 jobs.rooted [exact: own repository] permissions: not declared"
                .to_string(),
        ],
        "{scan}"
    );
    let publish = engine.context(PUBLISH, None).unwrap();
    assert_eq!(listed(&publish, "  consumer: ").len(), 4, "{publish}");
    for (job, name, mark) in [
        (
            ".github/workflows/case.yml::jobs.cased",
            "Publish.yml",
            "(absent at the indexed commit)",
        ),
        (
            ".github/workflows/ci.yml::jobs.internal",
            "internal.yml",
            "(absent at the indexed commit: the file is no reusable pipeline there)",
        ),
    ] {
        let answer = engine.context(job, None).unwrap();
        assert_eq!(
            listed(&answer, "consumes "),
            [format!(
                "{} @main permissions: not declared {mark}",
                file(name)
            )],
            "{answer}"
        );
        assert!(listed(&answer, "  provider: ").is_empty(), "{answer}");
    }
    let links = exported(&Engine::open(&app_a).unwrap());
    assert_eq!(
        pipelines(column(&links, "links", "contract")),
        [file("lint.yml"), file("publish.yml"), file("scan.yml")]
    );
    let levels = column(&links, "links", "level");
    assert!(levels.iter().all(|level| *level == "exact"), "{levels:?}");
}

#[test]
fn the_host_of_a_call_is_the_consumers_own() {
    let case = "ci_calls_hosts";
    let peer = "[[peer]]\nrepo = \"git.example.com/acme/workflows\"\n";
    let workflows = fixture_repo(
        case,
        "ci-workflows",
        Some("https://git.example.com/acme/workflows.git"),
    );
    let callers = [
        ("ci-app-a", Some("https://git.example.com/acme/app-a.git")),
        ("ci-app-b", Some("https://github.com/acme/app-b.git")),
        ("ci-wrapper", None),
    ]
    .map(|(fixture, origin)| {
        let repo = fixture_repo(case, fixture, origin);
        fs::write(repo.join("meada.toml"), peer).unwrap();
        repo
    });
    let engine = Engine::open(&workflows).unwrap();
    let answer = engine.context(PUBLISH, None).unwrap();
    let at = indexed(&workflows);
    assert_eq!(
        listed(&answer, "provides "),
        ["ci::git.example.com/acme/workflows/.github/workflows/publish.yml"],
        "{answer}"
    );
    assert_eq!(
        listed(&answer, "  consumer: "),
        [format!(
            "git.example.com/acme/app-a {CI}:12 jobs.publish [exact: @main (indexed {at})] with: package, registry; secrets: inherit; permissions: contents: read, id-token: write"
        )],
        "{answer}"
    );
    let [same, other, none] = callers.map(|repo| exported(&Engine::open(&repo).unwrap()));
    assert_eq!(
        pipelines(column(&same, "links", "contract")),
        ["lint.yml", "publish.yml", "scan.yml"]
            .map(|name| format!("ci::git.example.com/acme/workflows/.github/workflows/{name}"))
    );
    for export in [other, none] {
        assert!(column(&export, "consumers", "site").is_empty(), "{export}");
        assert!(column(&export, "links", "contract").is_empty(), "{export}");
    }
}

#[test]
fn a_reason_shows_the_ref_and_flags_only_another_full_commit_id() {
    let [workflows, _, _, _, app_c] = reproduction("ci_calls_refs");
    let head = git(&workflows, &["rev-parse", "HEAD"]).trim().to_string();
    let other = "0123456789abcdef0123456789abcdef01234567";
    assert_ne!(head, other);
    let refs = [
        ("same", head.clone(), ""),
        ("upper", head.to_uppercase(), ""),
        ("other", other.to_string(), ", ref mismatch"),
        ("short", other[..7].to_string(), ""),
        ("tag", "v1.2.3".to_string(), ""),
        ("branch", "release/2026".to_string(), ""),
    ];
    let jobs: String = (refs.iter())
        .map(|(id, written, _)| {
            format!("  {id}:\n    uses: acme/workflows/.github/workflows/publish.yml@{written}\n")
        })
        .collect();
    fs::write(
        app_c.join(".github/workflows/refs.yml"),
        format!("on: push\njobs:\n{jobs}"),
    )
    .unwrap();
    let engine = Engine::open(&workflows).unwrap();
    let answer = engine.context(PUBLISH, None).unwrap();
    let at = &head[..7];
    let mut expected: Vec<String> = (refs.iter().enumerate())
        .map(|(n, (id, written, flag))| {
            let line = 3 + 2 * n;
            format!(
                "github.com/acme/app-c .github/workflows/refs.yml:{line} jobs.{id} [exact: @{written} (indexed {at}){flag}] permissions: not declared"
            )
        })
        .collect();
    expected.sort();
    let mut shown: Vec<&str> = (listed(&answer, "  consumer: ").into_iter())
        .filter(|line| line.starts_with("github.com/acme/app-c "))
        .collect();
    shown.sort();
    assert_eq!(shown, expected, "{answer}");
    assert_eq!(listed(&answer, "provides "), [CONTRACT], "{answer}");
}

#[test]
fn a_uses_outside_the_workspace_holds_no_side_on_any_surface() {
    let [_, _, app_a, ..] = reproduction("ci_calls_external");
    let engine = Engine::open(&app_a).unwrap();
    let outside = ["actions/checkout", "docker/build-push-action", "other-org"];
    for address in [
        "jobs.build.steps[0]",
        "jobs.build.steps[1]",
        "jobs.elsewhere",
    ] {
        let answer = engine.context(&format!("{CI}::{address}"), None).unwrap();
        assert!(!sided(&answer), "{answer}");
    }
    let ambiguous = engine.search("", Some("ambiguous"), None).unwrap();
    assert_eq!(ambiguous, "no matches for ``\n");
    let unresolved = engine.search("", Some("unresolved"), None).unwrap();
    for line in unresolved.lines() {
        assert!(line.contains("(unresolved) candidates: 0; "), "{line}");
    }
    let export = engine.export().unwrap();
    let (status, _, ok) = meada(&app_a, &["status"], "", |_| {});
    assert!(ok, "{status}");
    for name in outside {
        for kind in ["contract", "ambiguous", "unresolved"] {
            let answer = engine.search(name, Some(kind), None).unwrap();
            assert_eq!(answer, format!("no matches for `{name}`\n"), "{kind}");
        }
        assert!(!export.contains(name), "{export}");
        assert!(!status.contains(name), "{status}");
    }
    assert_golden("ci-calls.status.txt", &status);
}

#[test]
fn context_on_a_long_pipeline_lists_its_callers_under_the_default_budget() {
    let [workflows, _, app_a, ..] = reproduction("ci_calls_long");
    let mut text = fs::read_to_string(workflows.join(PUBLISH)).unwrap();
    for n in 0..80 {
        text.push_str(&format!("      - name: Step {n}\n        run: echo {n}\n"));
    }
    fs::write(workflows.join(PUBLISH), text).unwrap();
    let engine = Engine::open(&app_a).unwrap();
    let answer = engine.context(PUBLISH, None).unwrap();
    assert!(
        answer.contains("raise budget"),
        "the answer is cut: {answer}"
    );
    assert_eq!(listed(&answer, "  consumer: ").len(), 4, "{answer}");
    let (sides, values) = (answer.find("provides ci::"), answer.find("\nvalues:"));
    assert!(sides < values, "{answer}");
    let answer = engine
        .context(&format!("{CI}::jobs.publish"), None)
        .unwrap();
    let (sides, values) = (answer.find("consumes ci::"), answer.find("\nvalues:"));
    assert!(sides.is_some() && sides < values, "{answer}");
}

#[test]
fn a_caller_indexed_alone_holds_no_side_until_its_peer_is_in_the_workspace() {
    let case = "ci_calls_alone";
    let app_b = fixture_repo(case, "ci-app-b", Some("https://github.com/acme/app-b.git"));
    let job = format!("{CI}::jobs[\"publish-prd\"]");
    let _ = fs::remove_dir_all(app_b.with_file_name("ci-workflows"));
    {
        let alone = Engine::open(&app_b).unwrap();
        let answer = alone.context(&job, None).unwrap();
        assert!(!sided(&answer), "{answer}");
        let export = alone.export().unwrap();
        assert!(!export.contains("ci::"), "{export}");
        let hits = alone.search("publish", Some("contract"), None).unwrap();
        assert_eq!(hits, "no matches for `publish`\n");
    }
    let workflows = fixture_repo(
        case,
        "ci-workflows",
        Some("https://github.com/acme/workflows.git"),
    );
    let engine = Engine::open(&app_b).unwrap();
    let answer = engine.context(&job, None).unwrap();
    let at = indexed(&workflows);
    assert_eq!(
        listed(&answer, "consumes "),
        [format!(
            "{CONTRACT} @v1 with: package; secrets: token; permissions: contents: read"
        )],
        "{answer}"
    );
    assert_eq!(
        listed(&answer, "  provider: "),
        [format!(
            "github.com/acme/workflows {PUBLISH}:1 . [exact: @v1 (indexed {at}), permissions mismatch: id-token write against none]"
        )],
        "{answer}"
    );
}

#[test]
fn the_reproduction_answers_and_exports_as_its_goldens() {
    let [workflows, wrapper, app_a, ..] = reproduction("ci_calls_goldens");
    let engine = Engine::open(&app_a).unwrap();
    let job = format!("{CI}::jobs.publish");
    let answers = [PUBLISH, &job, CONTRACT].map(|q| engine.context(q, None).unwrap());
    assert_golden("ci-calls.context.txt", &answers.join("\n"));
    let found = engine.search("publish", Some("contract"), None).unwrap();
    assert_golden("ci-calls.search.txt", &found);
    let default = engine.search("publish", None, None).unwrap();
    assert!(!default.contains("ci::"), "{default}");
    assert_golden("ci-calls.app-a.export.json", &engine.export().unwrap());
    for (name, repo) in [("workflows", &workflows), ("wrapper", &wrapper)] {
        let export = Engine::open(repo).unwrap().export().unwrap();
        assert_golden(&format!("ci-calls.{name}.export.json"), &export);
    }
}

#[test]
fn a_hand_declaration_keeps_its_declared_link_beside_the_ci_contract() {
    let [workflows, _, app_a, ..] = reproduction("ci_calls_declared");
    let declared = |role: &str, via: &str| {
        format!(
            "[[resource]]\nkind = \"gha-workflow\"\nid = \"acme/workflows/{PUBLISH}\"\nrole = \"{role}\"\nvia = [\"{via}\"]\n"
        )
    };
    fs::write(
        workflows.join("meada.toml"),
        declared(
            "write",
            &format!("{PUBLISH}#on.workflow_call.inputs.package.type"),
        ),
    )
    .unwrap();
    let peers = fs::read_to_string(app_a.join("meada.toml")).unwrap();
    let reads = declared("read", &format!("{CI}#jobs.publish.uses"));
    fs::write(app_a.join("meada.toml"), format!("{peers}\n{reads}")).unwrap();
    let export = exported(&Engine::open(&app_a).unwrap());
    let links: Vec<(&str, &str, &str, &str)> = (export["links"].as_array().unwrap().iter())
        .map(|l| {
            let text = |key: &str| l[key].as_str().unwrap();
            (
                text("contract"),
                text("level"),
                text("consumer"),
                text("provider"),
            )
        })
        .filter(|(contract, ..)| contract.ends_with("publish.yml"))
        .collect();
    assert_eq!(
        links,
        [
            (
                "resource::gha-workflow::acme/workflows/.github/workflows/publish.yml",
                "declared",
                "github.com/acme/app-a::.github/workflows/ci.yml#jobs.publish.uses",
                "github.com/acme/workflows::.github/workflows/publish.yml#on.workflow_call.inputs.package.type",
            ),
            (
                CONTRACT,
                "exact",
                "github.com/acme/app-a::.github/workflows/ci.yml::jobs.publish",
                "github.com/acme/workflows::.github/workflows/publish.yml::.",
            ),
        ],
        "{export}"
    );
}

#[test]
fn an_edited_call_or_provider_answers_as_a_cold_index_does() {
    let repos = reproduction("ci_calls_fresh");
    let [workflows, wrapper, ..] = &repos;
    let engine = Engine::open(workflows).unwrap();
    let before = engine.context(PUBLISH, None).unwrap();
    assert_eq!(listed(&before, "  consumer: ").len(), 4, "{before}");
    let release = wrapper.join(".github/workflows/release.yml");
    let text = fs::read_to_string(&release).unwrap();
    fs::write(&release, text.replace("publish.yml@main", "lint.yml@v3")).unwrap();
    let edited = engine.context(PUBLISH, None).unwrap();
    let consumers = listed(&edited, "  consumer: ");
    assert_eq!(consumers.len(), 3, "{edited}");
    assert!(!edited.contains("github.com/acme/wrapper"), "{edited}");
    let lint = engine.context(".github/workflows/lint.yml", None).unwrap();
    assert!(
        lint.contains("  consumer: github.com/acme/wrapper .github/workflows/release.yml:5 jobs.release [exact: @v3 "),
        "{lint}"
    );
    let publish = workflows.join(PUBLISH);
    let text = fs::read_to_string(&publish).unwrap();
    fs::write(
        &publish,
        text.replace("on:\n  workflow_call:", "on:\n  workflow_dispatch:"),
    )
    .unwrap();
    let answers = |engine: &Engine| {
        [
            PUBLISH,
            ".github/workflows/lint.yml",
            &format!("{CI}::jobs.publish"),
        ]
        .map(|q| engine.context(q, None).unwrap())
        .into_iter()
        .chain([engine.export().unwrap()])
        .collect::<Vec<_>>()
    };
    let warm = answers(&engine);
    assert!(
        !listed(&warm[0], "provides ").contains(&CONTRACT),
        "{}",
        warm[0]
    );
    assert!(listed(&warm[2], "  provider: ").is_empty(), "{}", warm[2]);
    drop(engine);
    for repo in &repos {
        fs::remove_dir_all(repo.join(".meada")).unwrap();
    }
    assert_eq!(answers(&Engine::open(workflows).unwrap()), warm);
}

pub(crate) fn traced(engine: &Engine, from: &str, direction: Direction) -> String {
    engine.trace(Start::From(from), direction, None).unwrap()
}

pub(crate) fn chain(answer: &str) -> Vec<&str> {
    let lines: Vec<&str> = answer.lines().collect();
    lines[..lines.len() - 1].to_vec()
}

#[test]
fn trace_follows_no_ci_call_against_its_direction() {
    let [workflows, _, _, app_b, _] = reproduction("ci_calls_trace_direction");
    let engine = Engine::open(&workflows).unwrap();
    let down = traced(&engine, PUBLISH, Direction::Down);
    assert_eq!(
        chain(&down),
        [
            format!("github.com/acme/workflows {PUBLISH}:1 ."),
            format!("  github.com/acme/workflows {PUBLISH}:15 jobs.publish"),
            format!("    github.com/acme/workflows {PUBLISH}:18 jobs.publish.steps[0]"),
            format!("    github.com/acme/workflows {PUBLISH}:19 jobs.publish.steps[1]"),
        ],
        "{down}"
    );
    let job = format!("github.com/acme/app-b::{CI}::jobs[\"publish-prd\"]");
    let up = traced(&Engine::open(&app_b).unwrap(), &job, Direction::Up);
    assert_eq!(
        chain(&up),
        [
            format!("github.com/acme/app-b {CI}:11 jobs[\"publish-prd\"]"),
            format!("  github.com/acme/app-b {CI}:1 ."),
        ],
        "{up}"
    );
}

#[test]
fn a_key_or_a_ref_with_a_control_character_writes_no_line_of_its_own() {
    let [workflows, _, app_a, ..] = reproduction("ci_calls_forged");
    let forged = "\\n  consumer: github.com/acme/forged x.yml:1 jobs.x [exact: @main]";
    fs::write(
        app_a.join(".github/workflows/forged.yml"),
        format!(
            "on: push\njobs:\n  keys:\n    uses: acme/workflows/{PUBLISH}@main\n    with:\n      \
             \"a{forged}\": 1\n      kept: 1\n    secrets:\n      \"b{forged}\": 1\n  ref:\n    \
             uses: \"acme/workflows/{PUBLISH}@main{forged}\"\n"
        ),
    )
    .unwrap();
    let engine = Engine::open(&workflows).unwrap();
    let answer = engine.context(PUBLISH, None).unwrap();
    assert!(!answer.contains("forged x.yml"), "{answer}");
    let consumers = listed(&answer, "  consumer: ");
    assert_eq!(consumers.len(), 5, "{answer}");
    let at = indexed(&workflows);
    assert!(
        consumers.contains(
            &format!(
                "github.com/acme/app-a .github/workflows/forged.yml:3 jobs.keys [exact: @main (indexed {at})] with: kept; permissions: not declared"
            )
            .as_str()
        ),
        "{answer}"
    );
    let job = engine
        .context(".github/workflows/forged.yml::jobs.ref", None)
        .unwrap();
    assert!(!sided(&job), "{job}");
}

#[test]
fn trace_up_from_a_reusable_pipeline_reaches_its_callers_through_an_intermediate_pipeline() {
    let [workflows, wrapper, ..] = reproduction("ci_calls_trace_up");
    let engine = Engine::open(&workflows).unwrap();
    let answer = traced(&engine, PUBLISH, Direction::Up);
    let (at, through) = (indexed(&workflows), indexed(&wrapper));
    let release = ".github/workflows/release.yml";
    assert_eq!(
        chain(&answer),
        [
            format!("github.com/acme/workflows {PUBLISH}:1 ."),
            format!("  →[exact: @main (indexed {at})] github.com/acme/app-a {CI}:12 jobs.publish"),
            format!("    github.com/acme/app-a {CI}:1 ."),
            format!(
                "  →[exact: @v1 (indexed {at}), permissions mismatch: id-token write against none] github.com/acme/app-b {CI}:11 jobs[\"publish-prd\"]"
            ),
            format!("    github.com/acme/app-b {CI}:1 ."),
            format!(
                "  →[exact: @main (indexed {at})] github.com/acme/app-b {CI}:18 jobs[\"publish-stg\"]"
            ),
            format!("    github.com/acme/app-b {CI}:1 . (see above)"),
            format!(
                "  →[exact: @main (indexed {at})] github.com/acme/wrapper {release}:5 jobs.release"
            ),
            format!("    github.com/acme/wrapper {release}:1 ."),
            format!(
                "      →[exact: @main (indexed {through})] github.com/acme/app-c {release}:6 jobs.release"
            ),
            format!("        github.com/acme/app-c {release}:1 ."),
        ],
        "{answer}"
    );
    let footer = answer.lines().last().unwrap();
    assert!(
        footer.starts_with("chain: exact · 5 hops · 5 repos · "),
        "{answer}"
    );
}

#[test]
fn trace_down_from_a_caller_reaches_the_pipeline_it_calls_through_the_intermediate_one() {
    let [workflows, wrapper, _, _, app_c] = reproduction("ci_calls_trace_down");
    let engine = Engine::open(&app_c).unwrap();
    let release = ".github/workflows/release.yml";
    let start = format!("github.com/acme/app-c::{release}::jobs.release");
    let answer = traced(&engine, &start, Direction::Down);
    let (at, through) = (indexed(&workflows), indexed(&wrapper));
    assert_eq!(
        chain(&answer),
        [
            format!("github.com/acme/app-c {release}:6 jobs.release"),
            format!("  →[exact: @main (indexed {through})] github.com/acme/wrapper {release}:1 ."),
            format!("    github.com/acme/wrapper {release}:5 jobs.release"),
            format!("      →[exact: @main (indexed {at})] github.com/acme/workflows {PUBLISH}:1 ."),
            format!("        github.com/acme/workflows {PUBLISH}:15 jobs.publish"),
            format!("          github.com/acme/workflows {PUBLISH}:18 jobs.publish.steps[0]"),
            format!("          github.com/acme/workflows {PUBLISH}:19 jobs.publish.steps[1]"),
        ],
        "{answer}"
    );
    let footer = answer.lines().last().unwrap();
    assert!(
        footer.starts_with("chain: exact · 2 hops · 3 repos · "),
        "{answer}"
    );
    assert_golden(
        "ci-calls.trace.txt",
        &format!(
            "{}\n{answer}",
            traced(&Engine::open(&workflows).unwrap(), PUBLISH, Direction::Up)
        ),
    );
}

#[test]
fn a_ci_contract_its_pipeline_node_and_its_file_path_start_the_same_chain() {
    let [workflows, ..] = reproduction("ci_calls_trace_starts");
    let engine = Engine::open(&workflows).unwrap();
    let node = format!("github.com/acme/workflows::{PUBLISH}::.");
    for direction in [Direction::Up, Direction::Down] {
        let by_path = traced(&engine, PUBLISH, direction);
        assert!(by_path.lines().count() > 2, "{by_path}");
        assert_eq!(traced(&engine, CONTRACT, direction), by_path);
        assert_eq!(traced(&engine, &node, direction), by_path);
    }
    let missing = traced(
        &engine,
        &CONTRACT.replace("publish", "absent"),
        Direction::Up,
    );
    assert!(
        missing.contains("is a ci, and a trace starts from"),
        "{missing}"
    );
}

#[test]
fn a_cycle_of_ci_calls_ends_at_the_walks_own_cycle_mark() {
    let [workflows, wrapper, ..] = reproduction("ci_calls_trace_cycle");
    let looped = |other: &str| {
        format!(
            "on: workflow_call\njobs:\n  again:\n    uses: acme/{other}/.github/workflows/loop.yml@main\n"
        )
    };
    fs::write(
        workflows.join(".github/workflows/loop.yml"),
        looped("wrapper"),
    )
    .unwrap();
    fs::write(
        wrapper.join(".github/workflows/loop.yml"),
        looped("workflows"),
    )
    .unwrap();
    let engine = Engine::open(&workflows).unwrap();
    let start = "github.com/acme/workflows::.github/workflows/loop.yml";
    for direction in [Direction::Up, Direction::Down] {
        let answer = traced(&engine, start, direction);
        let lines = chain(&answer);
        assert_eq!(lines.len(), 5, "{answer}");
        assert!(lines[4].ends_with("loop.yml:1 . (cycle)"), "{answer}");
        let cut = engine
            .trace(Start::From(start), direction, Some(60))
            .unwrap();
        assert!(cut.lines().count() < answer.lines().count(), "{cut}");
    }
}

#[test]
fn trace_reaches_a_caller_only_when_the_workspace_holds_its_repository() {
    let case = "ci_calls_trace_peers";
    let [workflows, ..] = reproduction(case);
    let stranger = fixture_repo(case, "ci-app-a", Some("https://github.com/acme/app-a.git"));
    fs::remove_file(stranger.join("meada.toml")).unwrap();
    let engine = Engine::open(&workflows).unwrap();
    let answer = traced(&engine, PUBLISH, Direction::Up);
    assert!(!answer.contains("acme/app-a"), "{answer}");
    assert!(answer.contains("acme/app-b"), "{answer}");
}
