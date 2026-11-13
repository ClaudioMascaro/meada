//! The CI contract (#824, ADR 0028): a CI job that calls a reusable pipeline of another
//! repository of the workspace is a consumer of `ci::<repository identity>/<path>`, and the
//! pipeline's file node is its provider.

use std::fs;
use std::path::{Path, PathBuf};

use meada::engine::{Direction, Engine, Start};
use serde_json::Value;

use crate::common::{assert_golden, fixture_repo, git, meada};

const PUBLISH: &str = ".github/workflows/publish.yml";
const CI: &str = ".github/workflows/ci.yml";
const CONTRACT: &str = "ci::github.com/acme/workflows/.github/workflows/publish.yml";

/// The words that open a line of a CI side in `context`.
const SIDE_WORDS: [&str; 4] = ["provides ", "consumes ", "  consumer: ", "  provider: "];

/// The reproduction of #816 as siblings in one case directory: the reusable pipelines
/// (`acme/workflows`), an intermediate reusable pipeline (`acme/wrapper`) and three callers.
pub(crate) fn reproduction(case: &str) -> [PathBuf; 5] {
    ["workflows", "wrapper", "app-a", "app-b", "app-c"].map(|name| {
        let origin = format!("https://github.com/acme/{name}.git");
        fixture_repo(case, &format!("ci-{name}"), Some(&origin))
    })
}

/// The indexed commit of the checkout at `root`, as a link's reason shows it.
pub(crate) fn indexed(root: &Path) -> String {
    git(root, &["rev-parse", "HEAD"])[..7].to_string()
}

/// The export of the repository where `engine` runs, as JSON.
pub(crate) fn exported(engine: &Engine) -> Value {
    serde_json::from_str(&engine.export().unwrap()).unwrap()
}

/// The texts at `key` of the rows of the section `section` of `export`.
fn column<'a>(export: &'a Value, section: &str, key: &str) -> Vec<&'a str> {
    (export[section].as_array().unwrap().iter())
        .map(|row| row[key].as_str().unwrap())
        .collect()
}

/// The contracts among `contracts` that name a pipeline file: the calls of CI jobs, without
/// those of the CI steps of `forms.yml` (#826, `ci_uses`).
fn pipelines(contracts: Vec<&str>) -> Vec<&str> {
    let pipeline = |contract: &&str| contract.contains("/.github/workflows/");
    contracts.into_iter().filter(pipeline).collect()
}

/// Whether `answer` holds a line of a CI side.
fn sided(answer: &str) -> bool {
    (answer.lines()).any(|line| SIDE_WORDS.iter().any(|word| line.starts_with(word)))
}

/// The lines of `answer` that start with `prefix`, without it.
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
    // Exactly four, #820's table: two CI jobs of one repository are two consumers, each with
    // the Git ref it wrote and the keys it passes.
    assert_eq!(
        listed(&answer, "  consumer: "),
        [
            format!(
                "github.com/acme/app-a {ci}:12 jobs.publish [exact: @main (indexed {at})] with: package, registry; secrets: inherit"
            ),
            format!(
                "github.com/acme/app-b {ci}:11 jobs[\"publish-prd\"] [exact: @v1 (indexed {at})] with: package; secrets: token"
            ),
            format!(
                "github.com/acme/app-b {ci}:18 jobs[\"publish-stg\"] [exact: @main (indexed {at})] with: package"
            ),
            format!(
                "github.com/acme/wrapper .github/workflows/release.yml:5 jobs.release [exact: @main (indexed {at})] with: package; secrets: inherit"
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

/// `on` holds `workflow_call` as a list (`lint.yml`), a map (`publish.yml`) and a string
/// (`scan.yml`); `internal.yml` runs on a push alone and provides nothing.
#[test]
fn a_pipeline_provides_only_with_workflow_call_and_its_identifier_is_lower_cased() {
    let [workflows, _, app_a, _, app_c] = reproduction("ci_calls_providers");
    // The path keeps its case: `Publish.yml` is another file than `publish.yml`.
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
    // `Acme/Workflows`, as app-a writes it, is `acme/workflows`: one contract.
    let scan = engine.context(".github/workflows/scan.yml", None).unwrap();
    let at = indexed(&workflows);
    assert_eq!(listed(&scan, "provides "), [file("scan.yml")], "{scan}");
    assert_eq!(
        listed(&scan, "  consumer: "),
        [
            format!("github.com/acme/app-a {CI}:24 jobs.scan [exact: @v2 (indexed {at})]"),
            // The `$/` call of `forms.yml` (#826), inside the repository: no ref.
            "github.com/acme/workflows .github/workflows/forms.yml:8 jobs.rooted [exact: own repository]"
                .to_string(),
        ],
        "{scan}"
    );
    let publish = engine.context(PUBLISH, None).unwrap();
    assert_eq!(listed(&publish, "  consumer: ").len(), 4, "{publish}");
    // A call to a file that provides nothing is a consumer no provider answers, marked absent
    // (#827): no file of that name, and a file that is there and is not reusable.
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
            [format!("{} @main {mark}", file(name))],
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

/// A `uses` writes no host: it is the one of the consumer's repository identity.
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
        // Another host (GitHub Connect) and no `origin`: neither names the peer.
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
            "git.example.com/acme/app-a {CI}:12 jobs.publish [exact: @main (indexed {at})] with: package, registry; secrets: inherit"
        )],
        "{answer}"
    );
    let [same, other, none] = callers.map(|repo| exported(&Engine::open(&repo).unwrap()));
    assert_eq!(
        pipelines(column(&same, "links", "contract")),
        ["lint.yml", "publish.yml", "scan.yml"]
            .map(|name| format!("ci::git.example.com/acme/workflows/.github/workflows/{name}"))
    );
    // The wrapper without `origin` still provides its own reusable pipeline, to local calls.
    for export in [other, none] {
        assert!(column(&export, "consumers", "site").is_empty(), "{export}");
        assert!(column(&export, "links", "contract").is_empty(), "{export}");
    }
}

/// The reason shows the Git ref as written and the provider's indexed commit, and flags only a
/// full commit id other than it. The level never depends on the ref.
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
        // An abbreviated commit id reads as a name, as a tag and a branch do.
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
                "github.com/acme/app-c .github/workflows/refs.yml:{line} jobs.{id} [exact: @{written} (indexed {at}){flag}]"
            )
        })
        .collect();
    expected.sort();
    let mut shown: Vec<&str> = (listed(&answer, "  consumer: ").into_iter())
        .filter(|line| line.starts_with("github.com/acme/app-c "))
        .collect();
    shown.sort();
    assert_eq!(shown, expected, "{answer}");
    // One contract for the file, whatever refs its consumers name.
    assert_eq!(listed(&answer, "provides "), [CONTRACT], "{answer}");
}

/// app-a's CI steps call `actions/checkout` and `docker/build-push-action`, and one of its CI
/// jobs a pipeline of `other-org`: none is in the workspace, so none holds a side.
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
    // The open items are the CI steps whose `./<folder>` is unresolved (#826), none of these.
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
    // The whole status: each repository and its config files, no line of a side, and one
    // orphan, the reusable pipeline `forms.yml` nothing calls (#827).
    assert_golden("ci-calls.status.txt", &status);
}

/// An index knows only its own repository: alone, a call names no repository of the workspace.
#[test]
fn a_caller_indexed_alone_holds_no_side_until_its_peer_is_in_the_workspace() {
    let case = "ci_calls_alone";
    let app_b = fixture_repo(case, "ci-app-b", Some("https://github.com/acme/app-b.git"));
    let job = format!("{CI}::jobs[\"publish-prd\"]");
    // An earlier run of this test left the peer beside it.
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
        [format!("{CONTRACT} @v1 with: package; secrets: token")],
        "{answer}"
    );
    assert_eq!(
        listed(&answer, "  provider: "),
        [format!(
            "github.com/acme/workflows {PUBLISH}:1 . [exact: @v1 (indexed {at})]"
        )],
        "{answer}"
    );
}

/// The answers and the exports of the reproduction, whole.
#[test]
fn the_reproduction_answers_and_exports_as_its_goldens() {
    let [workflows, wrapper, app_a, ..] = reproduction("ci_calls_goldens");
    let engine = Engine::open(&app_a).unwrap();
    let job = format!("{CI}::jobs.publish");
    let answers = [PUBLISH, &job, CONTRACT].map(|q| engine.context(q, None).unwrap());
    assert_golden("ci-calls.context.txt", &answers.join("\n"));
    let found = engine.search("publish", Some("contract"), None).unwrap();
    assert_golden("ci-calls.search.txt", &found);
    // The default search answers as before: the CI contracts crowd no search.
    let default = engine.search("publish", None, None).unwrap();
    assert!(!default.contains("ci::"), "{default}");
    assert_golden("ci-calls.app-a.export.json", &engine.export().unwrap());
    for (name, repo) in [("workflows", &workflows), ("wrapper", &wrapper)] {
        let export = Engine::open(repo).unwrap().export().unwrap();
        assert_golden(&format!("ci-calls.{name}.export.json"), &export);
    }
}

/// The hand declaration of #816 (`[[resource]]` with `via` on the keys of the call) keeps its
/// sides and its `declared` link beside the CI contract.
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

/// Each side is indexed in its own repository, so an edit to one file changes the answer after
/// the refresh a query makes, as a cold index gives it.
#[test]
fn an_edited_call_or_provider_answers_as_a_cold_index_does() {
    let repos = reproduction("ci_calls_fresh");
    let [workflows, wrapper, ..] = &repos;
    let engine = Engine::open(workflows).unwrap();
    let before = engine.context(PUBLISH, None).unwrap();
    assert_eq!(listed(&before, "  consumer: ").len(), 4, "{before}");
    // Only a consumer's `uses`: the wrapper now calls another pipeline.
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
    // Only the provider's file: the pipeline is reusable no more.
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

/// The lines of the chain in `answer`, without its footer.
pub(crate) fn chain(answer: &str) -> Vec<&str> {
    let lines: Vec<&str> = answer.lines().collect();
    lines[..lines.len() - 1].to_vec()
}

/// A CI job calls the pipeline, so the pipeline is no writer that goes on to its readers: down
/// from it are its own CI job and CI steps, and up from a caller is the pipeline of its file.
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

/// A key or a Git ref that holds a line break would write a line of its own in `context`
/// (found by the verifier of #824): such a key is no key, and such a `uses` names no pipeline.
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
                "github.com/acme/app-a .github/workflows/forged.yml:3 jobs.keys [exact: @main (indexed {at})] with: kept"
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
    // Each caller of #820's table, then the pipeline of its file; `acme/wrapper`'s file is a
    // provider itself, so the chain goes on to `acme/app-c`, which never names `publish.yml`.
    assert_eq!(
        chain(&answer),
        [
            format!("github.com/acme/workflows {PUBLISH}:1 ."),
            format!("  →[exact: @main (indexed {at})] github.com/acme/app-a {CI}:12 jobs.publish"),
            format!("    github.com/acme/app-a {CI}:1 ."),
            format!(
                "  →[exact: @v1 (indexed {at})] github.com/acme/app-b {CI}:11 jobs[\"publish-prd\"]"
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
    // The weakest level of the chain, its five links and its five repositories.
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
    // `jobs.release` alone fits the CI job of `acme/wrapper` too.
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
    // A contract no pipeline of the workspace provides is still no start.
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

/// Two reusable pipelines that call each other: GitHub refuses the run, the files still exist.
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
        // Pipeline, CI job, pipeline, CI job, and the start again: no level past it.
        assert_eq!(lines.len(), 5, "{answer}");
        assert!(lines[4].ends_with("loop.yml:1 . (cycle)"), "{answer}");
        // A budget below the chain cuts it, as it cuts every chain.
        let cut = engine
            .trace(Start::From(start), direction, Some(60))
            .unwrap();
        assert!(cut.lines().count() < answer.lines().count(), "{cut}");
    }
}

/// A sibling that calls the pipeline and that no `meada.toml` names is no part of the workspace.
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
