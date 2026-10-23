//! `context`'s result for one symbol and its answer: the head, the processes line, the span text
//! and the sections after it, cut under the token budget in a fixed order.

use std::collections::{BTreeSet, HashSet};

use crate::export::EntryKind;
use crate::render;
use crate::shape::{self, Shape};

/// A process a symbol belongs to: its entry point's repository, kind, label and identity.
pub struct Process<'a> {
    pub repository: &'a str,
    pub kind: EntryKind,
    pub label: &'a str,
    pub identity: &'a str,
}

/// What `context` found for one symbol, each part already placed: the first line
/// (`<repository> <path>:<start>-<end> <name>`), `community: <name>`, the processes it belongs
/// to, the span text's lines, and the lines after them (callers, callees, consumers, contracts).
pub struct Symbol<'a> {
    pub header: String,
    pub community: String,
    pub processes: Vec<Process<'a>>,
    pub span: Vec<String>,
    pub tail: Vec<String>,
}

impl Symbol<'_> {
    /// The answer cut at `budget` tokens, from the last to give up: the header, the community,
    /// the processes line (its summary, then as many entries as fit), the tail, and the span
    /// text, which keeps its leading lines and ends with the cut note of `render::cut`, or is
    /// left out when not even that note fits.
    pub fn answer(&self, budget: Option<usize>) -> String {
        let bytes = |lines: &[String]| lines.iter().map(|l| render::line_bytes(l)).sum::<usize>();
        let tokens = budget.unwrap_or(render::DEFAULT_BUDGET);
        let cap = render::bytes(tokens);
        let head = [self.header.clone(), self.community.clone()];
        // An answer that fits is cut nowhere, with every process named.
        let full = processes_line(&self.processes, usize::MAX);
        let whole: Vec<String> = (head.iter().cloned())
            .chain([full])
            .chain(self.span.iter().cloned())
            .chain(self.tail.iter().cloned())
            .collect();
        if bytes(&whole) <= cap {
            return render::budgeted(&whole, budget);
        }
        // The processes line may take what the head leaves less the widest note the final cut
        // can write, so that cut never reaches it.
        let total = head.len() + 1 + self.span.len() + 1 + self.tail.len();
        let widest = render::note(total, total, tokens);
        let room = cap.saturating_sub(bytes(&head) + 1 + widest.len());
        let processes = processes_line(&self.processes, room);
        let left = cap.saturating_sub(bytes(&head) + render::line_bytes(&processes));
        // The span gives up first: it keeps the lines the tail leaves, then its own note. With no
        // room even for the note it is left out, and the agent reads it from the lines the
        // header's range names.
        let span_room = left.saturating_sub(bytes(&self.tail));
        let (_, span) = render::cut(&self.span, span_room, tokens);
        let span = if span.len() > span_room {
            String::new()
        } else {
            span
        };
        let mut lines = Vec::from(head);
        lines.push(processes);
        lines.extend(span.lines().map(str::to_string));
        lines.extend(self.tail.iter().cloned());
        render::budgeted(&lines, budget)
    }
}

/// `processes: none`, or `processes: <n> (<k> <kind>, ...; <r> repositories)` for the processes
/// `found`, a summary present whenever there is one, then `: ` and the entries that fit in
/// `room` bytes, in order, and `, +<m> more` for the rest. When not even the first entry fits
/// the summary stands alone. An entry is `<label> (<repository>)`, or its `process::` identity
/// when another process of the same repository has the same label.
pub fn processes_line(found: &[Process], room: usize) -> String {
    if found.is_empty() {
        return "processes: none".into();
    }
    let kinds: Vec<String> = (EntryKind::ALL.iter())
        .filter_map(|k| {
            let n = found.iter().filter(|p| p.kind == *k).count();
            (n > 0).then(|| format!("{n} {}", k.name()))
        })
        .collect();
    let repositories = (found.iter().map(|p| p.repository).collect::<HashSet<_>>()).len();
    let summary = format!(
        "processes: {} ({}; {repositories} {})",
        found.len(),
        kinds.join(", "),
        if repositories == 1 {
            "repository"
        } else {
            "repositories"
        }
    );
    let entries: Vec<String> = (found.iter())
        .map(|p| {
            let same = found.iter().filter(|o| o.repository == p.repository);
            match same.filter(|o| o.label == p.label).count() {
                1 => format!("{} ({})", p.label, p.repository),
                _ => format!("process::{}", p.identity),
            }
        })
        .collect();
    let mut kept = Vec::new();
    // The length of the summary and the entries kept, joined.
    let mut used = summary.len() + ": ".len();
    for (i, entry) in entries.iter().enumerate() {
        let len = used + if kept.is_empty() { 0 } else { ", ".len() } + entry.len();
        let rest = entries.len() - i - 1;
        let more = if rest > 0 {
            format!(", +{rest} more").len()
        } else {
            0
        };
        if len + more > room {
            break;
        }
        kept.push(entry.clone());
        used = len;
    }
    if kept.is_empty() {
        return summary;
    }
    let rest = entries.len() - kept.len();
    if rest > 0 {
        kept.push(format!("+{rest} more"));
    }
    format!("{summary}: {}", kept.join(", "))
}

/// What a route's response (or a channel's payload: `what`) carries, after its `provides` line:
/// the fields and the parts left open, each line naming where the shape comes from.
pub fn shape_lines(identity: &str, shape: &Shape, what: &str) -> Vec<String> {
    let at = shape.source.at();
    let list = |paths: &BTreeSet<String>| {
        let paths: Vec<&str> = paths.iter().map(|p| shape::shown(p)).collect();
        paths.join(", ")
    };
    let fields = list(&shape.fields);
    let mut lines = vec![
        format!("{identity} {at} {what} fields: {fields}")
            .trim_end()
            .to_string(),
    ];
    if !shape.open.is_empty() {
        lines.push(format!(
            "{identity} {at} {what} open: {}",
            list(&shape.open)
        ));
    }
    lines
}
