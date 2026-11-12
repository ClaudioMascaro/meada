//! GitHub Actions as IaC nodes (#823, ADR 0028): a pipeline (a file of `.github/workflows/` that
//! holds `jobs`), an action (an `action.yml` or `action.yaml` that holds `runs`), each CI job and
//! each CI step, with its file and lines. A file is one evaluation unit, its root the file's
//! path, and only its first YAML document gives nodes. An address is the key path of the block
//! (ADR 0025): `.` for the file, `jobs.build`, `jobs["publish-prd"]`, `jobs.build.steps[2]`,
//! `runs.steps[0]`. References stay inside the file: a pipeline to its CI jobs, a CI job or a
//! composite action to its CI steps, a CI job to the CI jobs its `needs` names. A
//! `strategy.matrix` multiplies nothing: a CI job is one node.

use std::collections::{HashMap, HashSet};
use std::fmt::Write as _;

use rusqlite::{Connection, Transaction};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use super::cloudformation::scalar_text;
use super::compose::KeyLines;
use super::{NODES_MAX, Node, Owner, REFERENCES_MAX, ROOT_STORED_MAX, Reference};
use crate::Result;
use crate::config_values::{self, VALUE_MAX};
use crate::contract::Hit;
use crate::document::events::{self, Step, Syntax};
use crate::lang::MAX_BYTES;
use crate::render::rank;

/// Where GitHub reads pipelines from: the files of this folder, none below it.
const WORKFLOWS: &str = ".github/workflows/";

/// The node types of this format. Prose says pipeline, action, CI job and CI step (ADR 0028).
const WORKFLOW: &str = "workflow";
const ACTION: &str = "action";
const JOB: &str = "job";
const STEP: &str = "step";

/// The condition that a row of `iac_nodes` is a node of this format: its unit is its file, which
/// no Terraform root or Kubernetes unit is, and its type is one of the four.
pub(super) const ROWS: &str = "(root = path AND type IN ('workflow', 'action', 'job', 'step'))";

/// What a query names a CI job or a CI step by besides its address (ADR 0028): selectors, never
/// identity. A CI step's name is the node's own.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub(crate) struct Selectors {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uses: Option<String>,
}

/// Whether the file at `path` may be a pipeline or an action, by its place and name alone.
pub(crate) fn is_file(path: &str) -> bool {
    is_workflow(path) || action_folder(path).is_some()
}

fn is_workflow(path: &str) -> bool {
    path.strip_prefix(WORKFLOWS).is_some_and(|name| {
        !name.contains('/') && (name.ends_with(".yml") || name.ends_with(".yaml"))
    })
}

/// The folder (with its `/`, empty for the root) of an `action.yml` or `action.yaml`.
fn action_folder(path: &str) -> Option<&str> {
    let folder = (path.strip_suffix("action.yml")).or_else(|| path.strip_suffix("action.yaml"))?;
    (folder.is_empty() || folder.ends_with('/')).then_some(folder)
}

/// The other action file name of the folder of `path`, which is an action file by its name.
pub(crate) fn twin(path: &str) -> Option<String> {
    let folder = action_folder(path)?;
    let other = if path.ends_with(".yml") {
        "action.yaml"
    } else {
        "action.yml"
    };
    Some(format!("{folder}{other}"))
}

/// Whether `n` is a node of this format.
pub(crate) fn owns(n: &Node) -> bool {
    n.root == n.path && matches!(n.ty.as_str(), WORKFLOW | ACTION | JOB | STEP)
}

/// The line and the end mark of a mapping, as the event reader closes it.
type Lines = (usize, usize);

/// The lines of the CI jobs and CI steps of the first document, gathered as each mapping closes:
/// a CI step's before its CI job's. One an alias repeats closes nowhere and has none.
#[derive(Default)]
struct Spans {
    /// The CI steps closed since the last CI job did, by index.
    open: HashMap<usize, Lines>,
    jobs: HashMap<String, (Lines, HashMap<usize, Lines>)>,
    /// The CI steps of a composite action.
    runs: HashMap<usize, Lines>,
}

impl Spans {
    fn close(&mut self, c: &events::Collection<'_>) {
        if c.document != 0 {
            return;
        }
        let lines = (c.line, c.end_line);
        match c.path {
            [
                Step::Key(jobs),
                Step::Key(_),
                Step::Key(steps),
                Step::Index(n),
            ] if jobs == "jobs" && steps == "steps" => {
                self.open.insert(*n, lines);
            }
            [Step::Key(runs), Step::Key(steps), Step::Index(n)]
                if runs == "runs" && steps == "steps" =>
            {
                self.runs.insert(*n, lines);
            }
            [Step::Key(jobs), Step::Key(id)] if jobs == "jobs" => {
                let steps = std::mem::take(&mut self.open);
                self.jobs.insert(id.clone(), (lines, steps));
            }
            _ => {}
        }
    }
}

/// A scalar a query may match: text short enough to be a name, a number or a boolean as written.
fn text(value: Option<&Value>) -> Option<String> {
    scalar_text(value?).filter(|s| !s.is_empty() && s.len() <= VALUE_MAX)
}

/// The nodes and references of one file, within the ceilings of a unit.
struct Unit<'a> {
    path: &'a str,
    /// Whether the file holds several documents: its config values then start with `[0]`.
    several: bool,
    ends: KeyLines<'a>,
    nodes: Vec<Node>,
    references: Vec<Reference>,
    /// The bytes of addresses, names and selectors still to store (`ROOT_STORED_MAX`).
    left: usize,
}

impl Unit<'_> {
    fn take(&mut self, size: usize) -> bool {
        match self.left.checked_sub(size) {
            Some(left) if self.nodes.len() < NODES_MAX => {
                self.left = left;
                true
            }
            _ => false,
        }
    }

    /// Adds the node at `address` with the mapping `body`, located at `lines` (its first line
    /// and the reader's end mark), unless a ceiling is reached.
    fn node(&mut self, address: &str, ty: &str, body: &Map<String, Value>, lines: Lines) -> bool {
        let name = text(body.get("name"));
        let selectors = Selectors {
            id: text(body.get("id")),
            uses: text(body.get("uses")),
        };
        let size = [&name, &selectors.id, &selectors.uses]
            .iter()
            .map(|s| s.as_ref().map_or(0, String::len))
            .sum::<usize>();
        if !self.take(address.len() + size) {
            return false;
        }
        let block = match (self.several, address) {
            (false, ".") => String::new(),
            (false, _) => address.to_string(),
            (true, _) => config_values::prefixed(0, address),
        };
        let (line, mark) = lines;
        self.nodes.push(Node {
            address: address.to_string(),
            root: self.path.to_string(),
            ty: ty.to_string(),
            name,
            path: self.path.to_string(),
            line,
            // The mark is where the reader saw the mapping end, the next block's first line:
            // the last line above it indented deeper than the key is the block's own.
            end_line: self.ends.end_of(line, mark),
            block,
            selectors: (selectors != Selectors::default()).then_some(selectors),
            ..Node::default()
        });
        true
    }

    /// Adds the reference from `source` to `target`, unless `source` holds `REFERENCES_MAX` or
    /// the bytes are spent.
    fn reference(&mut self, source: &str, target: &str, via: Option<&str>, held: &mut usize) {
        let size = source.len() + target.len() + via.map_or(0, str::len);
        if *held < REFERENCES_MAX && self.take(size) {
            *held += 1;
            self.references.push(Reference {
                root: self.path.to_string(),
                source: source.to_string(),
                target: target.to_string(),
                via: via.map(str::to_string),
            });
        }
    }

    /// The CI steps of `body`, the mapping at `parent`, each a node that `source` references: the
    /// CI job itself, or the action whose `runs` holds them.
    fn steps(
        &mut self,
        source: &str,
        parent: &str,
        body: &Map<String, Value>,
        located: &HashMap<usize, Lines>,
        fallback: Lines,
        held: &mut usize,
    ) {
        let Some(Value::Array(steps)) = body.get("steps") else {
            return;
        };
        for (n, step) in steps.iter().enumerate() {
            let Value::Object(step) = step else {
                continue;
            };
            // Before the address is written: a long job id times many steps is the hostile case.
            if self.left < parent.len() {
                return;
            }
            let address = format!("{parent}.steps[{n}]");
            // A step an alias repeats sits at its parent's first line.
            let lines = located.get(&n).copied().unwrap_or((fallback.0, fallback.0));
            if !self.node(&address, STEP, step, lines) {
                return;
            }
            self.reference(source, &address, None, held);
        }
    }
}

/// The nodes and references of the pipeline or action at `path`: none for any other file, one
/// past the size ceiling or one that does not read.
pub(crate) fn evaluate(path: &str, src: &[u8]) -> (Vec<Node>, Vec<Reference>) {
    let none = (Vec::new(), Vec::new());
    if !is_file(path) || src.len() > MAX_BYTES {
        return none;
    }
    let Ok(text) = std::str::from_utf8(src) else {
        return none;
    };
    let mut spans = Spans::default();
    let read = events::read_located(Syntax::Yaml, src, |_| {}, |c, _| spans.close(&c));
    let Ok(documents) = read else {
        return none;
    };
    let Some(Value::Object(doc)) = documents.first() else {
        return none;
    };
    let ty = match () {
        _ if is_workflow(path) && doc.contains_key("jobs") => WORKFLOW,
        _ if action_folder(path).is_some() && doc.contains_key("runs") => ACTION,
        _ => return none,
    };
    let lines: Vec<&str> = text.lines().collect();
    let whole = (1, lines.len().max(1));
    let mut unit = Unit {
        path,
        several: documents.len() > 1,
        ends: KeyLines::new(&lines, "", &[]),
        nodes: Vec::new(),
        references: Vec::new(),
        left: ROOT_STORED_MAX,
    };
    // The file's own `id` and `uses` are no selector: only its name is kept.
    let named: Map<String, Value> = (doc.get("name").cloned())
        .map(|name| ("name".to_string(), name))
        .into_iter()
        .collect();
    if !unit.node(".", ty, &named, whole) {
        return none;
    }
    // The file node ends at the last line, whatever its indent.
    unit.nodes[0].end_line = whole.1;
    let mut held = 0;
    if ty == ACTION {
        if let Some(Value::Object(runs)) = doc.get("runs") {
            unit.steps(".", "runs", runs, &spans.runs, whole, &mut held);
        }
        return (unit.nodes, unit.references);
    }
    let Some(Value::Object(jobs)) = doc.get("jobs") else {
        return (unit.nodes, unit.references);
    };
    let address =
        |id: &str| config_values::key_path(&[Step::Key("jobs".into()), Step::Key(id.to_string())]);
    for (id, job) in jobs {
        let Value::Object(job) = job else {
            continue;
        };
        let at = address(id);
        let located = spans.jobs.get(id);
        let lines = located.map_or((1, 1), |(lines, _)| *lines);
        // A CI job is found by its id, which its address may quote.
        let mut body = job.clone();
        body.insert("id".into(), Value::String(id.clone()));
        if !unit.node(&at, JOB, &body, lines) {
            break;
        }
        unit.reference(".", &at, None, &mut held);
        let mut held = 0;
        let needs = match job.get("needs") {
            Some(Value::Array(needs)) => needs.iter().filter_map(Value::as_str).collect(),
            Some(Value::String(one)) => vec![one.as_str()],
            _ => Vec::new(),
        };
        // A CI job named twice is needed once.
        let mut seen = HashSet::new();
        for needed in needs {
            let other = matches!(jobs.get(needed), Some(Value::Object(_))) && needed != id;
            if other && seen.insert(needed) {
                unit.reference(&at, &address(needed), Some("needs"), &mut held);
            }
        }
        let none = HashMap::new();
        let steps = located.map_or(&none, |(_, steps)| steps);
        unit.steps(&at, &at, job, steps, lines, &mut held);
    }
    (unit.nodes, unit.references)
}

/// Replaces the stored nodes and references of the file at `path` with those of `src`, `None`
/// for a file that left. `shadowed` says an `action.yml` sits beside this `action.yaml`: the
/// action of the folder is that one, the file the runner loads, and this file gives none.
pub(crate) fn refresh(
    tx: &Transaction,
    path: &str,
    src: Option<&[u8]>,
    shadowed: bool,
) -> Result<()> {
    super::forget(tx, Some(path), Owner::Actions)?;
    let (nodes, references) = src.map_or_else(Default::default, |src| evaluate(path, src));
    if shadowed && nodes.first().is_some_and(|n| n.ty == ACTION) {
        return Ok(());
    }
    super::insert(tx, &nodes, &references)
}

/// The nodes of this format in `db` (repository `identity`) that `needle`, lowercased, matches
/// by address, type, name, `id` or `uses`, as `search` hits: `<identity> <path>:<line> <address>
/// (<type>[, id "<id>"][, name "<name>"][, uses "<uses>"])`, ordered as the IaC hits are.
pub(crate) fn hits(identity: &str, db: &Connection, needle: &str) -> Result<Vec<Hit>> {
    // ponytail: scans every node of this format; an indexed lookup waits for a workspace that
    // makes it slow.
    let sql = format!("SELECT {} FROM iac_nodes WHERE {ROWS}", super::COLUMNS);
    let mut statement = db.prepare(&sql)?;
    let mut hits = Vec::new();
    for n in statement.query_map([], super::row)? {
        let n = n?;
        let selectors = n.selectors.clone().unwrap_or_default();
        let shown = [
            ("id", &selectors.id),
            ("name", &n.name),
            ("uses", &selectors.uses),
        ];
        let fields = [Some(&n.address), Some(&n.ty)]
            .into_iter()
            .chain(shown.iter().map(|(_, value)| value.as_ref()));
        let Some(r) = (fields.flatten())
            .filter_map(|f| rank(&f.to_lowercase(), needle))
            .min()
        else {
            continue;
        };
        let mut text = format!("{} ({}", super::at(identity, &n), n.ty);
        for (word, value) in shown {
            if let Some(value) = value {
                let value = serde_json::to_string(value).unwrap_or_default();
                let _ = write!(text, ", {word} {value}");
            }
        }
        text.push(')');
        let key = format!("{identity} {} {:010} {}", n.path, n.line, n.address);
        hits.push((r, key, text));
    }
    Ok(hits)
}

/// Whether `text` names the node `n` of this format otherwise than by its address: the file node
/// by its path, alone or after the repository identity; a CI step by a selector in place of its
/// index (`jobs.build.steps[id=login]`, `name=` and `uses=` alike), alone, after the path or
/// after the identity and the path, as an address is (ADR 0025).
pub(super) fn names(identity: &str, n: &Node, text: &str) -> bool {
    if !owns(n) {
        return false;
    }
    let after = |text: &'_ str, prefix: &str| {
        (text.strip_prefix(prefix)).and_then(|rest| rest.strip_prefix("::").map(str::len))
    };
    let rest = after(text, identity);
    let text = rest.map_or(text, |len| &text[text.len() - len..]);
    if n.address == "." {
        return text == n.path;
    }
    let text = match (after(text, &n.path), rest) {
        (Some(len), _) => &text[text.len() - len..],
        (None, None) => text,
        (None, Some(_)) => return false,
    };
    let split = |address: &'_ str| {
        let (parent, rest) = address.strip_suffix(']')?.rsplit_once(".steps[")?;
        Some((parent.len(), rest.len()))
    };
    let (Some((parent, selector)), Some((own, _))) = (split(text), split(&n.address)) else {
        return false;
    };
    let selector = &text[text.len() - 1 - selector..text.len() - 1];
    let Some((key, value)) = selector.split_once('=') else {
        return false;
    };
    // A value may be quoted as a JSON string, as in a `via` selector (`config_values/sites.rs`).
    let quoted = serde_json::from_str::<String>(value).ok();
    let value = quoted
        .as_deref()
        .filter(|_| value.starts_with('"'))
        .unwrap_or(value);
    let selectors = n.selectors.as_ref();
    text[..parent] == n.address[..own]
        && match key {
            "id" => selectors.and_then(|s| s.id.as_deref()) == Some(value),
            "name" => n.name.as_deref() == Some(value),
            "uses" => selectors.and_then(|s| s.uses.as_deref()) == Some(value),
            _ => false,
        }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PIPELINE: &str = "\
name: CI
on:
  push:
jobs:
  build:
    name: Build
    runs-on: ubuntu-latest
    strategy:
      matrix:
        os: [linux, mac]
    steps:
      - uses: actions/checkout@v4
      - id: login
        name: Log in
        uses: docker/login-action@v3
        with:
          registry: ghcr.io
      - run: |
          make build
          make test

  publish-prd:
    needs: [build, absent, build]
    uses: acme/workflows/.github/workflows/publish.yml@v1
  test:
    needs: build
    steps:
      - run: make test
";

    fn shown(nodes: &[Node]) -> Vec<String> {
        (nodes.iter())
            .map(|n| format!("{} {} {}-{}", n.address, n.ty, n.line, n.end_line))
            .collect()
    }

    fn referenced(references: &[Reference]) -> Vec<String> {
        (references.iter())
            .map(|r| {
                let via = r
                    .via
                    .as_ref()
                    .map_or(String::new(), |v| format!(" via {v}"));
                format!("{} -> {}{via}", r.source, r.target)
            })
            .collect()
    }

    #[test]
    fn a_pipeline_gives_its_file_its_jobs_and_their_steps_as_nodes_at_their_key_paths() {
        let path = ".github/workflows/ci.yml";
        let (nodes, references) = evaluate(path, PIPELINE.as_bytes());
        assert_eq!(
            shown(&nodes),
            [
                ". workflow 1-28",
                "jobs.build job 5-20",
                "jobs.build.steps[0] step 12-12",
                "jobs.build.steps[1] step 13-17",
                "jobs.build.steps[2] step 18-20",
                "jobs[\"publish-prd\"] job 22-24",
                "jobs.test job 25-28",
                "jobs.test.steps[0] step 28-28",
            ]
        );
        assert!(nodes.iter().all(|n| n.root == path && n.path == path));
        assert_eq!(
            referenced(&references),
            [
                ". -> jobs.build",
                "jobs.build -> jobs.build.steps[0]",
                "jobs.build -> jobs.build.steps[1]",
                "jobs.build -> jobs.build.steps[2]",
                ". -> jobs[\"publish-prd\"]",
                "jobs[\"publish-prd\"] -> jobs.build via needs",
                ". -> jobs.test",
                "jobs.test -> jobs.build via needs",
                "jobs.test -> jobs.test.steps[0]",
            ]
        );
        // A block is the key path its config values start with; the file's is every value.
        assert_eq!(nodes[0].block, "");
        assert_eq!(nodes[3].block, "jobs.build.steps[1]");
        assert_eq!(nodes[0].name.as_deref(), Some("CI"));
        assert_eq!(nodes[1].name.as_deref(), Some("Build"));
        let login = nodes[3].selectors.as_ref().unwrap();
        assert_eq!(login.id.as_deref(), Some("login"));
        assert_eq!(login.uses.as_deref(), Some("docker/login-action@v3"));
        assert_eq!(nodes[3].name.as_deref(), Some("Log in"));
        let publish = nodes[5].selectors.as_ref().unwrap();
        assert_eq!(publish.id.as_deref(), Some("publish-prd"));
        assert!(publish.uses.as_deref().unwrap().ends_with("publish.yml@v1"));
    }

    #[test]
    fn an_action_gives_its_file_and_the_steps_of_its_runs() {
        let src = "name: Setup\nruns:\n  using: composite\n  steps:\n    - run: echo a\n      shell: bash\n    - uses: ./nested\n";
        for path in ["action.yml", "tools/setup/action.yaml"] {
            let (nodes, references) = evaluate(path, src.as_bytes());
            assert_eq!(
                shown(&nodes),
                [
                    ". action 1-7",
                    "runs.steps[0] step 5-6",
                    "runs.steps[1] step 7-7"
                ]
            );
            assert_eq!(
                referenced(&references),
                [". -> runs.steps[0]", ". -> runs.steps[1]"]
            );
        }
        // A Docker or JavaScript action has no steps: the file alone.
        let (nodes, _) = evaluate("action.yml", b"runs:\n  using: node20\n  main: index.js\n");
        assert_eq!(shown(&nodes), [". action 1-3"]);
    }

    #[test]
    fn only_a_pipeline_or_an_action_by_place_name_and_content_gives_nodes() {
        let pipeline = PIPELINE.as_bytes();
        let action = b"runs:\n  using: composite\n  steps: []\n";
        for (path, src) in [
            // A folder below `.github/workflows/` is not where GitHub reads pipelines.
            (".github/workflows/sub/ci.yml", pipeline),
            ("workflows/ci.yml", pipeline),
            (".github/workflows/ci.json", pipeline),
            (".github/workflows/ci.yml", &b"on: push\n"[..]),
            (".github/workflows/ci.yml", &b"- jobs\n"[..]),
            (".github/workflows/ci.yml", &b"jobs: [\n"[..]),
            ("my-action.yml", &action[..]),
            ("action.yml", &b"name: x\n"[..]),
            (".github/dependabot.yml", pipeline),
        ] {
            assert_eq!(evaluate(path, src).0, [], "{path}");
        }
        // An action file under the pipelines' folder that holds `jobs` is a pipeline.
        let (nodes, _) = evaluate(".github/workflows/action.yml", pipeline);
        assert_eq!(nodes[0].ty, WORKFLOW);
    }

    #[test]
    fn only_the_first_document_gives_nodes_and_its_blocks_follow_the_config_values() {
        let src = "jobs:\n  a:\n    steps:\n      - run: x\n---\njobs:\n  b: {}\n";
        let (nodes, _) = evaluate(".github/workflows/two.yml", src.as_bytes());
        assert_eq!(
            shown(&nodes),
            [
                ". workflow 1-7",
                "jobs.a job 2-4",
                "jobs.a.steps[0] step 4-4"
            ]
        );
        let blocks: Vec<_> = nodes.iter().map(|n| n.block.as_str()).collect();
        assert_eq!(blocks, ["[0]", "[0].jobs.a", "[0].jobs.a.steps[0]"]);
    }

    #[test]
    fn a_job_or_a_step_an_alias_repeats_is_a_node_at_its_parents_line() {
        let src = "jobs:\n  a: &a\n    steps:\n      - &s\n        run: x\n      - *s\n  b: *a\n";
        let (nodes, _) = evaluate(".github/workflows/alias.yml", src.as_bytes());
        assert_eq!(
            shown(&nodes),
            [
                ". workflow 1-7",
                "jobs.a job 2-6",
                "jobs.a.steps[0] step 5-5",
                "jobs.a.steps[1] step 2-2",
                "jobs.b job 1-1",
                "jobs.b.steps[0] step 1-1",
                "jobs.b.steps[1] step 1-1",
            ]
        );
    }

    #[test]
    fn a_unit_stops_at_its_ceilings() {
        // More steps than a unit holds nodes: the rest of the file is left out.
        let steps = "      - run: x\n".repeat(NODES_MAX + 5);
        let src = format!("jobs:\n  a:\n    steps:\n{steps}");
        let (nodes, _) = evaluate(".github/workflows/many.yml", src.as_bytes());
        assert_eq!(nodes.len(), NODES_MAX);
        // More steps than a node references: every step is a node, the first ones referenced.
        let steps = "      - run: x\n".repeat(REFERENCES_MAX + 5);
        let src = format!("jobs:\n  a:\n    steps:\n{steps}");
        let (nodes, references) = evaluate(".github/workflows/wide.yml", src.as_bytes());
        assert_eq!(nodes.len(), 2 + REFERENCES_MAX + 5);
        assert_eq!(references.len(), 1 + REFERENCES_MAX);
        // A long job id repeated by each of its steps stops at the bytes of a unit.
        let id = "j".repeat(MAX_BYTES / 2);
        let steps = "      - run: x\n".repeat(1000);
        let src = format!("jobs:\n  {id}:\n    steps:\n{steps}");
        let (nodes, references) = evaluate(".github/workflows/long.yml", src.as_bytes());
        let stored: usize = (nodes.iter().map(|n| n.address.len()))
            .chain(references.iter().map(|r| r.source.len() + r.target.len()))
            .sum();
        assert!(stored <= ROOT_STORED_MAX, "{stored}");
        assert!(nodes.len() < 10, "{}", nodes.len());
    }

    #[test]
    fn a_selector_or_the_file_path_names_a_node_and_is_no_identity() {
        let path = ".github/workflows/ci.yml";
        let (nodes, _) = evaluate(path, PIPELINE.as_bytes());
        let named = |text: &str| -> Vec<&str> {
            (nodes.iter())
                .filter(|n| names("example.com/acme/app", n, text))
                .map(|n| n.address.as_str())
                .collect()
        };
        assert_eq!(named(path), ["."]);
        assert_eq!(named(&format!("example.com/acme/app::{path}")), ["."]);
        for text in [
            "jobs.build.steps[id=login]",
            "jobs.build.steps[name=Log in]",
            "jobs.build.steps[name=\"Log in\"]",
            "jobs.build.steps[uses=docker/login-action@v3]",
            ".github/workflows/ci.yml::jobs.build.steps[id=login]",
            "example.com/acme/app::.github/workflows/ci.yml::jobs.build.steps[id=login]",
        ] {
            assert_eq!(named(text), ["jobs.build.steps[1]"], "{text}");
        }
        for text in [
            "jobs.test.steps[id=login]",
            "jobs.build.steps[id=absent]",
            "jobs.build.steps[run=make]",
            "other.yml::jobs.build.steps[id=login]",
            // After the repository a selector needs the path, as an address does (ADR 0025).
            "example.com/acme/app::jobs.build.steps[id=login]",
            "jobs.build",
        ] {
            assert_eq!(named(text), [""; 0], "{text}");
        }
    }
}
