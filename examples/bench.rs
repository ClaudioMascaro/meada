//! The benchmark over the public fixtures (spec #32): cold index time and size, single-file
//! reindex time, the tool-list token estimate and the token estimate of a fixed set of default
//! answers. A development harness, not a command: it adds nothing to the frozen surface.
//!
//! ```text
//! cargo build --release --locked --bin meada
//! cargo run --release --locked --example bench -- [--out <dir>]
//! ```
//!
//! It writes `bench.json` and `bench.md` to `<dir>` (default `target/bench`) and appends the
//! table to `$GITHUB_STEP_SUMMARY` when CI sets it. Timings are recorded, never judged: the
//! exit code is non-zero only when the harness itself fails (a fixture that does not index, an
//! answer that errors, the server that does not start). Token estimates are the trunk's
//! (`meada_core::render::tokens`) and are gated by the tests, not here. `MEADA_BENCH_RUNS`
//! (default 3) sets how many times each timing is taken; the median is reported. `MEADA_BIN` names
//! the `meada` binary the tool list is read from (default: the one beside this example's build).
//!
//! The scenarios: the chain of ADR 0003 (web, gateway, catalog and worker, one workspace), each
//! `conformance-*` fixture on its own, and each cross-language chain (`xlang-*` pairs, one
//! workspace each). Fixtures are copied to `<dir>/work/` and made repositories of their own, as
//! the tests do, so the checkout is never written.
//!
//! The default answers of a scenario are about its subject, the symbol of the repository where
//! Meada runs that is on the most calls, made or received (ties by identity): `search` by its
//! name, `context` and `trace` down and up from its identity, each at the default budget. The
//! table names the subject, so a jump in tokens that follows a change of subject reads as one.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use meada::engine::{Direction, Engine, Start};
use meada_core::render::tokens;
use rmcp::{
    ServiceExt,
    transport::{ConfigureCommandExt, TokioChildProcess},
};
use serde_json::{Value, json};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

/// The origin every conformance fixture is cloned from.
const CONFORMANCE: &str = "https://example.com/acme/conformance.git";

/// Conformance fixtures that are sibling repositories of another, not scenarios of their own:
/// the fixture, its origin and the fixture whose workspace it joins (`tests/it/conformance.rs`).
const SIBLINGS: &[(&str, &str, &str)] = &[(
    "conformance-go-ledger",
    "https://example.com/acme/ledger.git",
    "conformance-go",
)];

/// The chain of ADR 0003 (spec #3, #19): the repository where Meada runs first.
const CHAIN: &[(&str, &str)] = &[
    ("chain-web", "https://example.com/chain/web.git"),
    ("chain-gateway", "git@example.com:chain/gateway.git"),
    ("chain-catalog", "https://example.com/chain/catalog.git"),
    ("chain-worker", "https://example.com/chain/worker.git"),
];

/// The cross-language chains (`tests/it/common/mod.rs` builds the same pairs): the name of the
/// scenario, then the fixtures with their origins, the repository where Meada runs first.
type Pair = (&'static str, [(&'static str, &'static str); 2]);
const XLANG: &[Pair] = &[
    (
        "xlang-python",
        [
            (
                "xlang-storefront",
                "https://example.com/xlang/storefront.git",
            ),
            ("xlang-inventory", "git@example.com:xlang/inventory.git"),
        ],
    ),
    (
        "xlang-rust",
        [
            ("xlang-rust-web", "https://example.com/xlang-rust/web.git"),
            ("xlang-rust-api", "git@example.com:xlang-rust/api.git"),
        ],
    ),
    (
        "xlang-go",
        [
            ("xlang-go-web", "https://example.com/xlang-go/web.git"),
            ("xlang-go-api", "git@example.com:xlang-go/api.git"),
        ],
    ),
    (
        "xlang-java",
        [
            ("xlang-java-web", "https://example.com/xlang-java/web.git"),
            ("xlang-java-api", "git@example.com:xlang-java/api.git"),
        ],
    ),
    (
        "xlang-kotlin",
        [
            (
                "xlang-kotlin-web",
                "https://example.com/xlang-kotlin/web.git",
            ),
            ("xlang-kotlin-api", "git@example.com:xlang-kotlin/api.git"),
        ],
    ),
    (
        "xlang-csharp",
        [
            (
                "xlang-csharp-web",
                "https://example.com/xlang-csharp/web.git",
            ),
            ("xlang-csharp-api", "git@example.com:xlang-csharp/api.git"),
        ],
    ),
    (
        "xlang-swift",
        [
            ("xlang-swift-web", "https://example.com/xlang-swift/web.git"),
            ("xlang-swift-api", "git@example.com:xlang-swift/api.git"),
        ],
    ),
];

/// One workspace to measure: its name, and its fixtures with their origins, the repository
/// where Meada runs first.
struct Scenario {
    name: String,
    fixtures: Vec<(String, String)>,
}

/// What one scenario measured.
struct Row {
    name: String,
    repos: usize,
    files: usize,
    cold: Duration,
    size: u64,
    reindex: Duration,
    subject: String,
    subject_name: String,
    answers: Vec<(&'static str, usize)>,
}

fn main() {
    if let Err(e) = run() {
        eprintln!("bench: {e}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let mut out = PathBuf::from("target/bench");
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--out" => out = args.next().ok_or("--out takes a directory")?.into(),
            other => return Err(format!("unknown argument `{other}`").into()),
        }
    }
    let runs = match std::env::var("MEADA_BENCH_RUNS") {
        Ok(v) => v
            .parse::<usize>()
            .ok()
            .filter(|&n| n > 0)
            .ok_or("MEADA_BENCH_RUNS takes a positive number")?,
        Err(_) => 3,
    };
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let work = absolute(&out)?.join("work");

    let scenarios = scenarios(&root.join("tests/fixtures"))?;
    let mut rows = Vec::new();
    for scenario in &scenarios {
        let row =
            measure(root, &work, scenario, runs).map_err(|e| format!("{}: {e}", scenario.name))?;
        eprintln!("bench: {} done", row.name);
        rows.push(row);
    }
    // The server starts in the chain's repository where Meada runs, which the first scenario left.
    let first = &scenarios[0];
    let tool_list = measure_tool_list(&work.join(&first.name).join(&first.fixtures[0].0))?;

    fs::create_dir_all(&out)?;
    let markdown = markdown(&rows, tool_list, runs);
    fs::write(out.join("bench.md"), &markdown)?;
    fs::write(
        out.join("bench.json"),
        format!(
            "{}\n",
            serde_json::to_string_pretty(&report(&rows, tool_list, runs))?
        ),
    )?;
    if let Some(summary) = std::env::var_os("GITHUB_STEP_SUMMARY") {
        use std::io::Write;
        let mut file = fs::OpenOptions::new()
            .append(true)
            .create(true)
            .open(summary)?;
        write!(file, "{markdown}")?;
    }
    print!("{markdown}");
    Ok(())
}

fn absolute(path: &Path) -> Result<PathBuf> {
    Ok(std::env::current_dir()?.join(path))
}

/// Every scenario: the chain, the conformance fixtures found in `fixtures`, the cross-language
/// chains.
fn scenarios(fixtures: &Path) -> Result<Vec<Scenario>> {
    let own = |pairs: &[(&str, &str)]| -> Vec<(String, String)> {
        pairs
            .iter()
            .map(|&(f, o)| (f.to_string(), o.to_string()))
            .collect()
    };
    let mut all = vec![Scenario {
        name: "chain".into(),
        fixtures: own(CHAIN),
    }];
    let mut conformance: Vec<String> = fs::read_dir(fixtures)?
        .filter_map(|e| e.ok()?.file_name().into_string().ok())
        .filter(|n| n.starts_with("conformance-"))
        .collect();
    conformance.sort();
    for name in conformance {
        if SIBLINGS.iter().any(|&(sibling, _, _)| sibling == name) {
            continue;
        }
        let mut fixtures = vec![(name.clone(), CONFORMANCE.to_string())];
        for &(sibling, origin, of) in SIBLINGS {
            if of == name {
                fixtures.push((sibling.to_string(), origin.to_string()));
            }
        }
        all.push(Scenario { fixtures, name });
    }
    for (name, pair) in XLANG {
        all.push(Scenario {
            name: name.to_string(),
            fixtures: own(pair),
        });
    }
    Ok(all)
}

/// Copies the scenario's fixtures to `<work>/<scenario>/<fixture>` as repositories of their own,
/// and gives the directory where Meada runs.
fn prepare(root: &Path, work: &Path, scenario: &Scenario) -> Result<Vec<PathBuf>> {
    let base = work.join(&scenario.name);
    let _ = fs::remove_dir_all(&base);
    let mut dirs = Vec::new();
    for (fixture, origin) in &scenario.fixtures {
        let dir = base.join(fixture);
        copy_dir(&root.join("tests/fixtures").join(fixture), &dir)?;
        git(&dir, &["-c", "init.defaultBranch=main", "init", "-q"])?;
        git(&dir, &["remote", "add", "origin", origin])?;
        git(&dir, &["add", "-A"])?;
        git(&dir, &["commit", "-q", "-m", "fixture"])?;
        dirs.push(dir);
    }
    Ok(dirs)
}

fn copy_dir(from: &Path, to: &Path) -> Result<()> {
    fs::create_dir_all(to)?;
    for entry in fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir(&entry.path(), &target)?;
        } else {
            fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}

fn git(dir: &Path, args: &[&str]) -> Result<()> {
    let out = Command::new("git")
        .current_dir(dir)
        .args(args)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_AUTHOR_NAME", "Meada")
        .env("GIT_AUTHOR_EMAIL", "fixtures@example.com")
        .env("GIT_AUTHOR_DATE", "2026-01-01T00:00:00Z")
        .env("GIT_COMMITTER_NAME", "Meada")
        .env("GIT_COMMITTER_EMAIL", "fixtures@example.com")
        .env("GIT_COMMITTER_DATE", "2026-01-01T00:00:00Z")
        .output()?;
    if !out.status.success() {
        return Err(format!(
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&out.stderr)
        )
        .into());
    }
    Ok(())
}

/// Measures one scenario: `runs` cold indexes of fresh copies (the last one is kept), then
/// `runs` single-file reindexes and the default answers on it.
fn measure(root: &Path, work: &Path, scenario: &Scenario, runs: usize) -> Result<Row> {
    let mut cold = Vec::new();
    let mut kept = None;
    for _ in 0..runs {
        drop(kept.take());
        let dirs = prepare(root, work, scenario)?;
        let started = Instant::now();
        let engine = Engine::open(&dirs[0])?;
        engine.refresh()?;
        cold.push(started.elapsed());
        kept = Some((dirs, engine));
    }
    let (dirs, engine) = kept.ok_or("no run")?;
    let size = dirs
        .iter()
        .map(|d| dir_size(&d.join(".meada")))
        .sum::<std::io::Result<u64>>()?;

    let export: Value = serde_json::from_str(&engine.export()?)?;
    let files = export["files"].as_array().map_or(0, Vec::len);
    let (identity, path, name) = subject(&export)?;
    let answers = vec![
        ("search", tokens(&engine.search(&name, None, None)?)),
        ("context", tokens(&engine.context(&identity, None)?)),
        (
            "trace down",
            tokens(&engine.trace(Start::From(&identity), Direction::Down, None)?),
        ),
        (
            "trace up",
            tokens(&engine.trace(Start::From(&identity), Direction::Up, None)?),
        ),
    ];

    // A trailing newline changes the file's size, so the refresh reads it again.
    let edited = dirs[0].join(&path);
    let mut reindex = Vec::new();
    for _ in 0..runs {
        let mut text = fs::read(&edited)?;
        text.push(b'\n');
        fs::write(&edited, text)?;
        let started = Instant::now();
        engine.refresh()?;
        reindex.push(started.elapsed());
    }

    Ok(Row {
        name: scenario.name.clone(),
        repos: dirs.len(),
        files,
        cold: median(&mut cold),
        size,
        reindex: median(&mut reindex),
        subject: identity,
        subject_name: name,
        answers,
    })
}

/// The symbol of `export` that is on the most calls, as caller or as target (ties: the least
/// identity), as its identity, the path of its file and its name.
fn subject(export: &Value) -> Result<(String, String, String)> {
    let repository = export["repository"]
        .as_str()
        .ok_or("export has no repository")?;
    let mut on: BTreeMap<&str, usize> = BTreeMap::new();
    for call in export["calls"].as_array().into_iter().flatten() {
        for end in [&call["caller"], &call["target"]] {
            if let Some(symbol) = end.as_str() {
                *on.entry(symbol).or_default() += 1;
            }
        }
    }
    let mut best: Option<(&Value, usize)> = None;
    for symbol in export["symbols"].as_array().into_iter().flatten() {
        let calls = on
            .get(symbol["id"].as_str().unwrap_or(""))
            .copied()
            .unwrap_or(0);
        // Symbols come sorted by identity: a later one wins only with more calls.
        if best.is_none_or(|(_, most)| calls > most) {
            best = Some((symbol, calls));
        }
    }
    let (symbol, _) = best.ok_or("the fixture has no symbol")?;
    let identity = symbol["id"].as_str().ok_or("symbol without id")?;
    let path = symbol["span"]["path"]
        .as_str()
        .ok_or("symbol without path")?;
    let name = (identity.strip_prefix(&format!("{repository}::{path}::")))
        .ok_or("symbol identity is not <repository>::<path>::<name>")?;
    Ok((identity.into(), path.into(), name.into()))
}

fn median(times: &mut [Duration]) -> Duration {
    times.sort();
    times[times.len() / 2]
}

fn dir_size(dir: &Path) -> std::io::Result<u64> {
    let mut total = 0;
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        total += if entry.file_type()?.is_dir() {
            dir_size(&entry.path())?
        } else {
            entry.metadata()?.len()
        };
    }
    Ok(total)
}

/// The tool-list estimate, as the MCP smoke test takes it (`tests/it/mcp.rs`): what a client
/// reads of each tool, its description and its parameter schema, through `meada mcp` started in
/// `repo`.
fn measure_tool_list(repo: &Path) -> Result<usize> {
    let binary = match std::env::var_os("MEADA_BIN") {
        Some(path) => PathBuf::from(path),
        // target/<profile>/examples/bench sits beside target/<profile>/meada.
        None => std::env::current_exe()?
            .parent()
            .and_then(Path::parent)
            .ok_or("no directory beside the example")?
            .join("meada"),
    };
    if !binary.is_file() {
        return Err(format!(
            "no meada binary at {}: `cargo build --bin meada` first, or set MEADA_BIN",
            binary.display()
        )
        .into());
    }
    let repo = repo.to_path_buf();
    let runtime = tokio::runtime::Runtime::new()?;
    runtime.block_on(async move {
        let transport =
            TokioChildProcess::new(tokio::process::Command::new(binary).configure(|c| {
                c.arg("mcp").current_dir(repo);
            }))?;
        let client = ().serve(transport).await?;
        let listed = client.list_all_tools().await?;
        let mut text = String::new();
        for tool in &listed {
            text.push_str(tool.description.as_deref().unwrap_or(""));
            text.push_str(&serde_json::to_string(&*tool.input_schema)?);
        }
        client.cancel().await?;
        Ok(tokens(&text))
    })
}

fn ms(time: Duration) -> f64 {
    (time.as_secs_f64() * 1000.0 * 10.0).round() / 10.0
}

fn kib(bytes: u64) -> f64 {
    (bytes as f64 / 1024.0 * 10.0).round() / 10.0
}

/// A line naming the build and the machine the timings come from: they compare only with a run
/// on the same one.
fn environment() -> String {
    let cpus = std::thread::available_parallelism().map_or(0, |n| n.get());
    let build = if cfg!(debug_assertions) {
        "debug build, timings not comparable"
    } else {
        "release build"
    };
    format!(
        "{} {}, {cpus} CPUs, {build}",
        std::env::consts::OS,
        std::env::consts::ARCH
    )
}

fn markdown(rows: &[Row], tool_list: usize, runs: usize) -> String {
    let mut out = String::from("## Benchmark over the public fixtures\n\n");
    out.push_str(&format!(
        "Median of {runs} runs on {}. Timings are recorded, never gated: shared runners are noisy.\n\n",
        environment()
    ));
    out.push_str(&format!("Tool list: {tool_list} tokens.\n\n"));
    let names: Vec<&str> = (rows[0].answers.iter()).map(|(name, _)| *name).collect();
    out.push_str(
        "| Scenario | Repos | Files | Cold index (ms) | Index size (KiB) | Reindex (ms) | Subject |",
    );
    for name in &names {
        out.push_str(&format!(" {name} |"));
    }
    out.push_str("\n| --- | ---: | ---: | ---: | ---: | ---: | --- |");
    out.push_str(&" ---: |".repeat(names.len()));
    out.push('\n');
    for row in rows {
        out.push_str(&format!(
            "| {} | {} | {} | {} | {} | {} | `{}` |",
            row.name,
            row.repos,
            row.files,
            ms(row.cold),
            kib(row.size),
            ms(row.reindex),
            row.subject_name.replace('|', "\\|")
        ));
        for (_, tokens) in &row.answers {
            out.push_str(&format!(" {tokens} |"));
        }
        out.push('\n');
    }
    out.push_str(
        "\nFiles are the indexed files of the repository where Meada runs; the index size sums \
         every repository of the scenario. The columns after the subject (the symbol on the most \
         calls) are the estimated tokens of the default answers about it.\n",
    );
    out
}

fn report(rows: &[Row], tool_list: usize, runs: usize) -> Value {
    let scenarios: Vec<Value> = rows
        .iter()
        .map(|row| {
            json!({
                "scenario": row.name,
                "repos": row.repos,
                "files": row.files,
                "cold_index_ms": ms(row.cold),
                "index_bytes": row.size,
                "reindex_ms": ms(row.reindex),
                "subject": row.subject,
                "answer_tokens": row.answers.iter().cloned().collect::<BTreeMap<_, _>>(),
            })
        })
        .collect();
    json!({
        "runs": runs,
        "environment": environment(),
        "commit": std::env::var("GITHUB_SHA").ok(),
        "tool_list_tokens": tool_list,
        "scenarios": scenarios,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fixture left out of `XLANG` would never be measured: every cross-language fixture is
    /// in a pair, and every pair's fixtures exist.
    #[test]
    fn every_cross_language_fixture_is_measured() {
        let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
        let listed: Vec<&str> = (XLANG.iter().flat_map(|(_, pair)| pair.iter()))
            .map(|(fixture, _)| *fixture)
            .collect();
        for fixture in &listed {
            assert!(fixtures.join(fixture).is_dir(), "{fixture}");
        }
        for entry in fs::read_dir(&fixtures).unwrap() {
            let name = entry.unwrap().file_name().into_string().unwrap();
            if name.starts_with("xlang-") {
                assert!(listed.contains(&name.as_str()), "{name} is not in XLANG");
            }
        }
    }

    #[test]
    fn the_scenarios_hold_the_chain_every_conformance_fixture_and_every_pair() {
        let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
        let all = scenarios(&fixtures).unwrap();
        assert_eq!(all[0].name, "chain");
        assert_eq!(all[0].fixtures[0].0, "chain-web");
        let conformance = fs::read_dir(&fixtures)
            .unwrap()
            .filter(|e| {
                (e.as_ref().unwrap().file_name().to_str().unwrap()).starts_with("conformance-")
            })
            .count();
        assert_eq!(all.len(), 1 + conformance - SIBLINGS.len() + XLANG.len());
        let go = all.iter().find(|s| s.name == "conformance-go").unwrap();
        assert_eq!(go.fixtures[1].0, "conformance-go-ledger");
    }

    #[test]
    fn the_subject_is_the_symbol_on_the_most_calls() {
        let export = json!({
            "repository": "example.com/x",
            "symbols": [
                {"id": "example.com/x::a.ts::a", "span": {"path": "a.ts"}},
                {"id": "example.com/x::b.ts::m::b", "span": {"path": "b.ts"}},
                {"id": "example.com/x::c.ts::c", "span": {"path": "c.ts"}},
            ],
            "calls": [
                {"caller": "example.com/x::a.ts::a", "target": "example.com/x::b.ts::m::b"},
                {"caller": "example.com/x::b.ts::m::b", "target": null},
                {"caller": "example.com/x::b.ts::m::b", "target": null},
                {"caller": "example.com/x::c.ts::c", "target": "example.com/x::a.ts::a"},
                {"caller": "example.com/x::c.ts::c", "target": null},
            ],
        });
        // b makes two and receives one; a and c are on two each.
        let (identity, path, name) = subject(&export).unwrap();
        assert_eq!(identity, "example.com/x::b.ts::m::b");
        assert_eq!((path.as_str(), name.as_str()), ("b.ts", "m::b"));
    }
}
