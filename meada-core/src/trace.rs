//! `trace`'s result and its answer: the chain's lines, cut under the token budget and closed by
//! the chain footer (ADR 0008), with the peers the workspace names and cannot find.

use std::collections::HashSet;

use crate::export::{Level, Link};
use crate::render;

/// One line of the chain.
pub struct Row {
    pub text: String,
    /// The position of the repository the line is in.
    pub repo: usize,
    /// The level of the call or link that reached it.
    pub level: Option<Level>,
    pub link: bool,
    /// What ends the line once it is complete: ` drift: missing <paths>` after a link whose
    /// consumer reads fields its provider lacks (spec #23).
    pub end: String,
}

impl Row {
    /// A line reached through a call at `level`, or a line with no level.
    pub fn call(text: String, repo: usize, level: Option<Level>) -> Self {
        Self {
            text,
            repo,
            level,
            link: false,
            end: String::new(),
        }
    }

    /// A line reached through `link`.
    pub fn link(text: String, repo: usize, link: &Link) -> Self {
        let missing = link.drift.iter().flat_map(|d| d.missing.keys());
        let missing: Vec<&str> = missing.map(String::as_str).collect();
        Self {
            text,
            repo,
            level: Some(link.level),
            link: true,
            end: match missing.is_empty() {
                true => String::new(),
                false => format!(" drift: missing {}", missing.join(", ")),
            },
        }
    }
}

/// A chain walked from its start: its rows in order, and one line per missing peer.
pub struct Chain {
    pub rows: Vec<Row>,
    pub missing: Vec<String>,
}

impl Chain {
    /// The rows that fit `budget` with room for the footer, the cut note if any, then the footer
    /// (`chain: <weakest level> · <hops> hops · <repos> repos · <tokens> tokens`) and the missing
    /// peers.
    pub fn answer(&self, budget: Option<usize>) -> String {
        let budget = budget.unwrap_or(render::DEFAULT_BUDGET);
        let footer = |weakest: Option<Level>, hops: usize, repos: usize, tokens: usize| {
            let weakest = weakest.map_or("none", Level::name);
            let mut text =
                format!("chain: {weakest} · {hops} hops · {repos} repos · {tokens} tokens\n");
            for line in &self.missing {
                text += line;
                text += "\n";
            }
            text
        };
        let stats = |rows: &[Row]| {
            let weakest = rows.iter().filter_map(|r| r.level).max();
            let hops = rows.iter().filter(|r| r.link).count();
            let repos = rows.iter().map(|r| r.repo).collect::<HashSet<_>>().len();
            (weakest, hops, repos)
        };
        // Room for the widest footer the shown rows can give: an answer within budget uses at
        // most `budget` tokens.
        let (_, hops, repos) = stats(&self.rows);
        let widest = footer(Some(Level::Ambiguous), hops, repos, budget);
        let lines: Vec<String> = self.rows.iter().map(|r| r.text.clone()).collect();
        let room = budget.saturating_sub(render::tokens(&widest));
        let (shown, body) = render::cut(&lines, render::bytes(room), budget);
        let (weakest, hops, repos) = stats(&self.rows[..shown]);
        // The footer counts its own tokens: settle on the count that includes its digits.
        let mut tokens = 0;
        loop {
            let answer = body.clone() + &footer(weakest, hops, repos, tokens);
            match render::tokens(&answer) {
                t if t == tokens => return answer,
                t => tokens = t,
            }
        }
    }
}

/// ` [<level>]` after a process step reached by a call that is not exact. Unlike
/// `render::mark`, it names no reason: a step keeps the call's level, not the call.
pub fn level_mark(level: Option<Level>) -> String {
    match level {
        Some(Level::Exact) | None => String::new(),
        Some(level) => format!(" [{}]", level.name()),
    }
}

/// `→[<level>: <reason>]`.
pub fn arrow(link: &Link) -> String {
    format!("→[{}: {}]", link.level.name(), link.reason)
}
