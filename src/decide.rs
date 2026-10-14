//! `meada decide` (spec #18, ADR 0006): resolves the open items of the repository where Meada
//! runs and records the picks in `meada.decisions.toml`. Human mode asks on a terminal, or reads
//! the same lines from a script; it reads no environment variable and opens no connection.

use std::io::{BufRead, Write};

use crate::Result;
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
        let what = match c.method() {
            "" => c.contract.clone(),
            method => format!("{method} {}", c.path()),
        };
        writeln!(out, "  {}) {} {what}", n + 1, c.repository)?;
    }
    Ok(())
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

/// Runs the command in human mode: `--list` shows the items; otherwise it asks, then writes
/// the file once, at the end (or prints it for `--dry-run`). Nothing is written when nothing
/// was picked.
pub fn run(
    engine: &Engine,
    options: &Options,
    input: &mut impl BufRead,
    out: &mut impl Write,
) -> Result<()> {
    let items = items(engine, options.item.as_deref())?;
    if items.is_empty() {
        writeln!(out, "{}", Msg::DecideNoItems.text())?;
        return Ok(());
    }
    if options.list {
        for (n, item) in items.iter().enumerate() {
            if n > 0 {
                writeln!(out)?;
            }
            show(item, out)?;
        }
        return Ok(());
    }
    let picks = ask(&items, input, out)?;
    finish(engine, picks, options.dry_run, out)
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
