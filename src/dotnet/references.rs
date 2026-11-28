use std::collections::HashSet;

use crate::repo::relative;

// ponytail: real solutions nest a few projects deep; this bounds the walk over a hostile one.
pub const REFERENCE_DEPTH: usize = 32;

pub fn located(dir: &str, include: &str) -> Option<String> {
    if include.contains("$(") || include.contains(':') {
        return None;
    }
    relative(dir, &include.replace('\\', "/")).filter(|at| at.ends_with(".csproj"))
}

pub fn referenced<'p>(
    start: &'p str,
    references: impl Fn(&str) -> Vec<(&'p str, bool)>,
) -> Vec<&'p str> {
    let mut seen = HashSet::from([start]);
    let (mut found, mut level) = (Vec::new(), vec![start]);
    for _ in 0..REFERENCE_DEPTH {
        let mut next = Vec::new();
        for project in level {
            for (to, private) in references(project) {
                if (!private || project == start) && seen.insert(to) {
                    next.push(to);
                }
            }
        }
        if next.is_empty() {
            break;
        }
        found.extend(&next);
        level = next;
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn walk<'p>(graph: &HashMap<&'p str, Vec<(&'p str, bool)>>, start: &'p str) -> Vec<&'p str> {
        referenced(start, |p| graph.get(p).cloned().unwrap_or_default())
    }

    #[test]
    fn a_reference_is_located_from_the_project_that_writes_it() {
        assert_eq!(
            located("src/App", "..\\Lib\\Lib.csproj").as_deref(),
            Some("src/Lib/Lib.csproj")
        );
        assert_eq!(
            located("src/App", "../../../other/Money.csproj").as_deref(),
            Some("../other/Money.csproj")
        );
        for include in ["$(Root)\\X.csproj", "C:\\abs\\X.csproj", "..\\X\\X.vbproj"] {
            assert_eq!(located("src/App", include), None, "{include}");
        }
    }

    #[test]
    fn references_are_followed_nearest_first_and_a_private_one_from_the_start_only() {
        let graph = HashMap::from([
            ("App", vec![("Mid", true), ("Kit", false)]),
            (
                "Mid",
                vec![("Deep", false), ("Hidden", true), ("Kit", false)],
            ),
            ("Deep", vec![("Deeper", false)]),
        ]);
        assert_eq!(walk(&graph, "App"), ["Mid", "Kit", "Deep", "Deeper"]);
        assert_eq!(walk(&graph, "Mid"), ["Deep", "Hidden", "Kit", "Deeper"]);
        assert!(walk(&graph, "Deeper").is_empty());
    }

    #[test]
    fn a_reference_cycle_and_a_chain_past_the_depth_bound_are_walked_in_linear_time() {
        let names: Vec<String> = (0..1000).map(|i| format!("P{i}")).collect();
        let all: Vec<(&str, bool)> = names.iter().map(|n| (n.as_str(), false)).collect();
        let reads = std::cell::Cell::new(0);
        let start = std::time::Instant::now();
        let found = referenced("P0", |_| {
            reads.set(reads.get() + 1);
            all.clone()
        });
        assert!(start.elapsed().as_secs() < 6, "{:?}", start.elapsed());
        assert_eq!((found.len(), reads.get()), (names.len() - 1, names.len()));
        let cycle = HashMap::from([("A", vec![("B", false)]), ("B", vec![("A", false)])]);
        assert_eq!(walk(&cycle, "A"), ["B"]);
        let line: HashMap<&str, Vec<(&str, bool)>> = (1..names.len())
            .map(|i| (names[i - 1].as_str(), vec![(names[i].as_str(), false)]))
            .collect();
        let walked = walk(&line, "P0");
        assert_eq!(walked.len(), REFERENCE_DEPTH);
        assert_eq!(walked.last(), Some(&names[REFERENCE_DEPTH].as_str()));
    }
}
