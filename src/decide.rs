//! `meada decide` (spec #18, ADR 0006): resolves the open items of the repository where Meada
//! runs and records the picks in `meada.decisions.toml`. Human mode asks on a terminal, or reads
//! the same lines from a script; it reads no environment variable and opens no connection.
//! Decider mode (`--decider`, `crate::decider`) asks a model instead.

use std::io::{BufRead, Write};

use crate::Result;
use crate::decider;
use crate::decisions::{self, Decision};
use crate::engine::{Candidate, Engine, OpenItem};
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

/// The open items of this repository to work on, in id order; with `only`, that one item.
pub(crate) fn items(engine: &Engine, only: Option<&str>) -> Result<Vec<OpenItem>> {
    let mut items = engine.local_items()?;
    if let Some(id) = only {
        items.retain(|i| i.id == id);
        if items.is_empty() {
            return Err(Msg::DecideUnknownItem(id).text().into());
        }
    }
    Ok(items)
}

/// The item as `--list` and the prompt show it: id, location and call, then every candidate
/// numbered from 1.
fn show(item: &OpenItem, out: &mut impl Write) -> Result<()> {
    writeln!(out, "{}", item.id)?;
    writeln!(out, "{}  {}", item.location(), item.expression)?;
    for (n, c) in item.candidates.iter().enumerate() {
        writeln!(out, "  {}) {}", n + 1, describe(c))?;
    }
    Ok(())
}

/// A candidate as a person or a decider sees it: repository identity, method and path.
pub(crate) fn describe(c: &Candidate) -> String {
    match c.method() {
        "" => format!("{} {}", c.repository, c.contract),
        method => format!("{} {method} {}", c.repository, c.path()),
    }
}

/// Asks about each of `items` on `out` and reads the answers from `input`: a number picks that
/// candidate, `s` or an empty line skips. End of input ends the run with the picks so far.
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
        show(item, out)?;
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
/// `--decider`, then writes the file once, at the end (or prints it for `--dry-run`). Nothing
/// is written when nothing was picked. Only decider mode reads the decider variables, and
/// `--list` reads none.
pub fn run(
    engine: &Engine,
    options: &Options,
    input: &mut impl BufRead,
    out: &mut impl Write,
) -> Result<()> {
    // Checked whenever it is given, whatever else the run does.
    let min_p = options.min_p.map_or(Ok(decider::MIN_P), decider::min_p)?;
    if options.prune {
        return prune(engine, out);
    }
    let config = match options.decider && !options.list {
        true => Some((decider::Config::from_env()?, min_p)),
        false => None,
    };
    let items = items(engine, options.item.as_deref())?;
    if options.list {
        return list(engine, &items, out);
    }
    if items.is_empty() {
        writeln!(out, "{}", Msg::DecideNoItems.text())?;
        return Ok(());
    }
    let (picks, failed) = match &config {
        Some((config, min_p)) => {
            let (items, show) = (&items, options.show_request);
            let outcome = decider::run(config, engine.root(), items, show, *min_p, out)?;
            if show {
                return Ok(());
            }
            (outcome.decisions, outcome.failed)
        }
        None => (ask(&items, input, out)?, 0),
    };
    finish(engine, picks, options.dry_run, out)?;
    // The decisions of the other items are written; the status tells a script some failed.
    match failed {
        0 => Ok(()),
        n => Err(Msg::DecideFailures(n).text().into()),
    }
}

/// `--list`: the open items, then the stale decisions and those waiting on a missing peer.
fn list(engine: &Engine, items: &[OpenItem], out: &mut impl Write) -> Result<()> {
    if items.is_empty() {
        writeln!(out, "{}", Msg::DecideNoItems.text())?;
    }
    for (n, item) in items.iter().enumerate() {
        if n > 0 {
            writeln!(out)?;
        }
        show(item, out)?;
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

/// `--prune`: rewrites the file without its stale entries, in canonical form. It asks nothing
/// and reads no network; a decision waiting on a missing peer stays.
fn prune(engine: &Engine, out: &mut impl Write) -> Result<()> {
    let stale = engine.local_decisions()?.stale;
    match decisions::pruned(engine.root(), &stale)? {
        Some((text, removed)) => {
            decisions::save(engine.root(), &text)?;
            writeln!(out, "{}", Msg::DecidePruned(removed).text())?;
        }
        None => writeln!(out, "{}", Msg::DecideNothingToPrune.text())?,
    }
    Ok(())
}

/// Records `picks` in the repository's decisions file, or prints what the file would be.
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
    let text = decisions::canonical(engine.root(), picks)?;
    if dry_run {
        writeln!(out, "{}", Msg::DecideDryRun.text())?;
        write!(out, "{text}")?;
    } else {
        decisions::save(engine.root(), &text)?;
        writeln!(out, "{}", Msg::DecideWrote(count).text())?;
    }
    Ok(())
}
