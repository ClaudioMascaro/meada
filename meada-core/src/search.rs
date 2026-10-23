//! `search`'s result and its answer: hits ranked against the query, one line each.

use crate::render;

/// One match: `(rank, tie-break key, line)`. The rank is `render::rank`'s, so exact matches come
/// first, then prefix, then substring; the key orders hits of one rank.
pub type Hit = (u8, String, String);

/// The `hits` for `query` sorted and cut at `budget`, or the answer for none.
pub fn answer(query: &str, mut hits: Vec<Hit>, budget: Option<usize>) -> String {
    hits.sort();
    if hits.is_empty() {
        return format!("no matches for `{query}`\n");
    }
    let lines: Vec<String> = hits.into_iter().map(|(_, _, line)| line).collect();
    render::budgeted(&lines, budget)
}
