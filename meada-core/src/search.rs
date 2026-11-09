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

/// `answer` for hits whose lines `line` makes from a payload only once they are needed: lines
/// past the first that crosses `budget` are never made. Hits that tie on rank and key are
/// ordered by their lines, as `answer` orders them.
pub fn answer_by<T, E>(
    query: &str,
    mut hits: Vec<(u8, String, T)>,
    budget: Option<usize>,
    mut line: impl FnMut(&T) -> Result<String, E>,
) -> Result<String, E> {
    if hits.is_empty() {
        return Ok(format!("no matches for `{query}`\n"));
    }
    hits.sort_by(|a, b| (a.0, &a.1).cmp(&(b.0, &b.1)));
    let budget = budget.unwrap_or(render::DEFAULT_BUDGET);
    let cap = render::bytes(budget);
    let (mut lines, mut used) = (Vec::new(), 0);
    for tied in hits.chunk_by(|a, b| (a.0, &a.1) == (b.0, &b.1)) {
        if used > cap {
            break;
        }
        let mut made = tied
            .iter()
            .map(|h| line(&h.2))
            .collect::<Result<Vec<_>, E>>()?;
        made.sort();
        used += made.iter().map(|l| render::line_bytes(l)).sum::<usize>();
        lines.extend(made);
    }
    Ok(render::cut_of(&lines, hits.len(), cap, budget).1)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `answer_by` over the payloads `line` makes for the hits `answer` takes made.
    fn lazy(hits: &[Hit], budget: Option<usize>, made: &mut usize) -> String {
        let payloads = hits.iter().map(|(r, k, l)| (*r, k.clone(), l.clone()));
        answer_by("q", payloads.collect(), budget, |l: &String| {
            *made += 1;
            Ok::<_, ()>(l.clone())
        })
        .unwrap()
    }

    #[test]
    fn a_cut_answer_makes_only_the_lines_it_shows() {
        // Pairs tie on rank and key, and their lines sort against the order they come in.
        let hits: Vec<Hit> = (0..1000)
            .map(|i| {
                let line = format!("line {:04} {}", 999 - i, "x".repeat(40));
                ((i / 2 % 3) as u8, format!("k{:04}", i / 2), line)
            })
            .collect();
        let mut made = 0;
        assert_eq!(
            lazy(&hits, Some(100), &mut made),
            answer("q", hits.clone(), Some(100))
        );
        assert!(made <= 12, "{made} lines made for a 100-token answer");
        let mut made = 0;
        let all = Some(1_000_000);
        assert_eq!(lazy(&hits, all, &mut made), answer("q", hits.clone(), all));
        assert_eq!(made, 1000);
        assert_eq!(lazy(&[], None, &mut made), "no matches for `q`\n");
    }
}
