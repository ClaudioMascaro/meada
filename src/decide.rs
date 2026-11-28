//! `meada decide` (spec #18, ADR 0006): resolves the open items of the repository where Meada
//! runs and records the picks in `meada.decisions.toml`. Human mode asks on a terminal, or reads
//! the same lines from a script; it reads no environment variable and opens no connection.
//! Decider mode (`--decider`, `crate::decider`) asks a model instead.

use std::io::{BufRead, Write};
use std::path::Path;

use crate::Result;
use crate::decider;
use crate::decisions::{self, Decision};
use crate::engine::{Engine, OpenItem};
use crate::msg::Msg;

/// What the command line asked for.
#[derive(Default)]
pub struct Options {
    /// Work on this one item only.
    pub item: Option<String>,
    /// Print the decisions the run would write and leave the file as it is.
    pub dry_run: bool,
    /// Print the open items and neither ask nor write.
    pub list: bool,
    /// Remove the stale decisions from the file, and do nothing else.
    pub prune: bool,
    /// Ask the model the `MEADA_DECIDER_*` variables configure instead of a person.
    pub decider: bool,
    /// Decider mode: print every request the run would send, and send none.
    pub show_request: bool,
    /// Decider mode: write a model decision only from this probability up; `MIN_P` when unset.
    pub min_p: Option<f64>,
}

impl Options {
    fn can_save(&self) -> bool {
        self.prune || !(self.list || self.dry_run || self.show_request)
    }
}

/// Opens the workspace for decide, checking the local decisions file before discovery reads it.
pub fn open(dir: &Path, options: &Options) -> Result<Engine> {
    if options.can_save() {
        let root = crate::repo::root(dir)?;
        if options.decider && !options.prune {
            decisions::check_destination(&root)?;
        } else {
            decisions::writable(&root)?;
        }
    }
    Engine::open(dir)
}

pub(crate) fn items(engine: &Engine, only: Option<&str>) -> Result<Vec<OpenItem>> {
    let mut items = match only {
        Some(id) => vec![
            engine
                .local_item(id)?
                .ok_or_else(|| Msg::DecideUnknownItem(id).text())?,
        ],
        None => engine.local_items()?,
    };
    for item in &mut items {
        item.rank_candidates();
    }
    Ok(items)
}

/// ponytail: the full item listing, prompt, export and MCP have their own output contracts.
const LIST_CANDIDATE_LIMIT: usize = 5;

fn show(item: &OpenItem, out: &mut impl Write, overview: bool) -> Result<()> {
    writeln!(out, "{}", item.id)?;
    writeln!(out, "{}  {}", item.location(), item.expression)?;
    writeln!(out, r#"  kind = "{}""#, item.decision_kind().name())?;
    if let Some(cause) = &item.cause {
        writeln!(out, "  {cause}")?;
    }
    if let Some(limit) = &item.limit {
        writeln!(out, "  {}", Msg::ItemLimit(limit).text())?;
    }
    let count = item.candidates.len();
    let id = &item.id;
    if overview && item.literal_coverage == Some(0) && count > 0 {
        writeln!(out, "  {}", Msg::CandidateCount { count, id }.text())?;
        return Ok(());
    }
    let shown = if overview {
        count.min(LIST_CANDIDATE_LIMIT)
    } else {
        count
    };
    for (n, c) in item.candidates[..shown].iter().enumerate() {
        writeln!(out, "  {}) {}", n + 1, c.describe())?;
    }
    if count > shown {
        writeln!(
            out,
            "  {}",
            Msg::CandidatesMore {
                count: count - shown,
                id
            }
            .text()
        )?;
    }
    Ok(())
}

pub(crate) fn ask(
    items: &[OpenItem],
    input: &mut impl BufRead,
    out: &mut impl Write,
) -> Result<Vec<Decision>> {
    let mut picks = Vec::new();
    let mut line = String::new();
    'items: for (n, item) in items.iter().enumerate() {
        if n > 0 {
            writeln!(out)?;
        }
        show(item, out, false)?;
        let count = item.candidates.len();
        if count == 0 {
            writeln!(out, "{}", Msg::DecideNoCandidates.text())?;
            continue;
        }
        writeln!(out, "{}", Msg::DecidePrompt(count).text())?;
        loop {
            line.clear();
            out.flush()?;
            if input.read_line(&mut line)? == 0 {
                break 'items;
            }
            match line.trim() {
                "" | "s" => break,
                answer => match answer.parse::<usize>() {
                    Ok(pick) if (1..=count).contains(&pick) => {
                        picks.push(Decision::human(item, &item.candidates[pick - 1]));
                        break;
                    }
                    _ => writeln!(out, "{}", Msg::DecideRetry(count).text())?,
                },
            }
        }
    }
    Ok(picks)
}

/// Runs the command: `--list` shows the items; otherwise it asks a person, or the decider with
/// `--decider`. A model answer is saved on acceptance, and interactive picks are saved
/// together. `--dry-run` prints the resulting file. A run that can write refuses a decisions file that is a
/// symbolic link before it asks or sends anything (`decisions::writable`). Only decider mode
/// reads the decider variables, and `--list` reads none.
pub fn run(
    engine: &Engine,
    options: &Options,
    input: &mut impl BufRead,
    out: &mut impl Write,
) -> Result<()> {
    let min_p = options.min_p.map_or(Ok(decider::MIN_P), decider::min_p)?;
    if options.can_save() {
        decisions::writable(engine.root())?;
    }
    if options.prune {
        return prune(engine, out);
    }
    let config = match options.decider && !options.list {
        true => Some((decider::Config::from_env()?, min_p)),
        false => None,
    };
    let items = items(engine, options.item.as_deref())?;
    if options.list {
        return list(engine, &items, options.item.is_none(), out);
    }
    if items.is_empty() {
        writeln!(out, "{}", Msg::DecideNoItems.text())?;
        return Ok(());
    }
    let failed = match &config {
        Some((config, min_p)) => {
            let show = options.show_request;
            if !show && !options.dry_run {
                decisions::locked(engine.root(), || Ok(()))?;
            }
            let stored = |path: &str| engine.stored_setting(path);
            let accept = |decision: &Decision| {
                if options.dry_run {
                    return Ok(());
                }
                decisions::record(engine.root(), vec![decision.clone()])
            };
            let outcome = decider::run(
                config,
                engine.root(),
                &stored,
                &items,
                show,
                *min_p,
                accept,
                out,
            )?;
            if show {
                return Ok(());
            }
            if options.dry_run {
                finish(engine, outcome.decisions, true, out)?;
            } else {
                writeln!(out)?;
                let count = outcome.decisions.len();
                let message = if count == 0 {
                    Msg::DecideNothing
                } else {
                    Msg::DecideWrote(count)
                };
                writeln!(out, "{}", message.text())?;
            }
            outcome.failed
        }
        None => {
            finish(engine, ask(&items, input, out)?, options.dry_run, out)?;
            0
        }
    };
    match failed {
        0 => Ok(()),
        n => Err(Msg::DecideFailures(n).text().into()),
    }
}

fn list(engine: &Engine, items: &[OpenItem], overview: bool, out: &mut impl Write) -> Result<()> {
    if items.is_empty() {
        writeln!(out, "{}", Msg::DecideNoItems.text())?;
    }
    for (n, item) in items.iter().enumerate() {
        if n > 0 {
            writeln!(out)?;
        }
        show(item, out, overview)?;
    }
    let here = engine.local_decisions()?;
    if !here.stale.is_empty() || !here.waiting.is_empty() {
        writeln!(out)?;
    }
    for line in here.lines() {
        writeln!(out, "{line}")?;
    }
    Ok(())
}

fn prune(engine: &Engine, out: &mut impl Write) -> Result<()> {
    let stale = engine.local_decisions()?.stale;
    decisions::locked(engine.root(), || {
        match decisions::pruned(engine.root(), &stale)? {
            Some((text, removed)) => {
                decisions::save(engine.root(), &text)?;
                writeln!(out, "{}", Msg::DecidePruned(removed).text())?;
            }
            None => writeln!(out, "{}", Msg::DecideNothingToPrune.text())?,
        }
        Ok(())
    })?;
    Ok(())
}

pub(crate) fn finish(
    engine: &Engine,
    picks: Vec<Decision>,
    dry_run: bool,
    out: &mut impl Write,
) -> Result<()> {
    writeln!(out)?;
    if picks.is_empty() {
        writeln!(out, "{}", Msg::DecideNothing.text())?;
        return Ok(());
    }
    let count = picks.len();
    if dry_run {
        let text = decisions::canonical(engine.root(), picks)?;
        writeln!(out, "{}", Msg::DecideDryRun.text())?;
        write!(out, "{text}")?;
    } else {
        decisions::record(engine.root(), picks)?;
        writeln!(out, "{}", Msg::DecideWrote(count).text())?;
    }
    Ok(())
}
