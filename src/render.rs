//! Compact answers: line-oriented text under a token budget (ADR 0008). Every query shares
//! this, so the CLI and each MCP tool cut answers the same way.

/// Tokens an answer may use unless the caller asks for another budget.
pub const DEFAULT_BUDGET: usize = 2_000;

/// `lines` joined by newlines. When they exceed `budget` tokens (`DEFAULT_BUDGET` when
/// `None`; one token is four bytes of UTF-8, so the estimate is deterministic), the answer
/// keeps the lines that fit and ends with a line saying where it stopped.
pub fn budgeted(lines: &[String], budget: Option<usize>) -> String {
    let budget = budget.unwrap_or(DEFAULT_BUDGET);
    cut(lines, budget, budget).1
}

/// The estimated tokens of `text`: one per four bytes, rounded up.
pub fn tokens(text: &str) -> usize {
    text.len().div_ceil(4)
}

/// `lines` within `room` tokens, cut as `budgeted` cuts them, with the cut note naming
/// `budget`, and how many lines it kept. `room` is below `budget` when the caller appends
/// lines of its own.
pub fn cut(lines: &[String], room: usize, budget: usize) -> (usize, String) {
    let cap = room * 4;
    let all: usize = lines.iter().map(|l| l.len() + 1).sum();
    let note = |shown: usize| {
        format!(
            "… cut after {shown} of {} lines (budget {budget} tokens); raise budget for more\n",
            lines.len()
        )
    };
    let mut shown = lines.len();
    if all > cap {
        // The note's length depends on `shown`'s digits: reserve for the widest.
        let room = cap.saturating_sub(note(lines.len()).len());
        let mut used = 0;
        shown = lines
            .iter()
            .take_while(|l| {
                used += l.len() + 1;
                used <= room
            })
            .count();
    }
    let mut out: String = lines[..shown]
        .iter()
        .flat_map(|l| [l.as_str(), "\n"])
        .collect();
    if shown < lines.len() {
        out.push_str(&note(shown));
    }
    (shown, out)
}
