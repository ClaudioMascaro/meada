use std::collections::{BTreeMap, BTreeSet, VecDeque};

pub(crate) use meada_core::export::Community;

pub(crate) fn communities<'a>(
    calls: impl IntoIterator<Item = (&'a str, &'a str)>,
    dir: impl Fn(&str) -> Option<String>,
) -> Vec<Community> {
    let mut nodes: BTreeSet<&str> = BTreeSet::new();
    let mut weights: BTreeMap<(&str, &str), i128> = BTreeMap::new();
    for (a, b) in calls {
        let (a_is, b_is) = (dir(a).is_some(), dir(b).is_some());
        nodes.extend(
            [(a, a_is), (b, b_is)]
                .into_iter()
                .filter(|p| p.1)
                .map(|p| p.0),
        );
        if a != b && a_is && b_is {
            *weights.entry((a.min(b), a.max(b))).or_default() += 1;
        }
    }
    let ids: Vec<&str> = nodes.into_iter().collect();
    let at = |id: &str| ids.binary_search(&id).unwrap();
    let mut graph = Network {
        adj: vec![BTreeMap::new(); ids.len()],
        k: vec![0; ids.len()],
    };
    for (&(a, b), &w) in &weights {
        let (a, b) = (at(a), at(b));
        graph.adj[a].insert(b, w);
        graph.adj[b].insert(a, w);
        graph.k[a] += w;
        graph.k[b] += w;
    }
    let groups = leiden(graph);

    let natural: Vec<String> = (groups.iter())
        .map(|members| {
            let mut count: BTreeMap<String, usize> = BTreeMap::new();
            for &m in members {
                *count.entry(dir(ids[m]).unwrap()).or_default() += 1;
            }
            let most = count.values().max().copied().unwrap_or_default();
            count.into_iter().find(|(_, n)| *n == most).unwrap().0
        })
        .collect();
    let reserved: BTreeSet<&String> = natural.iter().collect();
    let mut taken: BTreeSet<String> = BTreeSet::new();
    let mut named: Vec<Community> = (groups.into_iter().zip(&natural))
        .map(|(members, name)| {
            let name = match taken.contains(name) {
                false => name.clone(),
                true => (2..)
                    .map(|n| format!("{name}#{n}"))
                    .find(|n| !taken.contains(n) && !reserved.contains(n))
                    .unwrap(),
            };
            taken.insert(name.clone());
            let mut community = Community::default();
            community.name = name;
            community.members = members.into_iter().map(|m| ids[m].to_string()).collect();
            community
        })
        .collect();
    named.sort_by(|a, b| a.name.cmp(&b.name));
    named
}

struct Network {
    adj: Vec<BTreeMap<usize, i128>>,
    k: Vec<i128>,
}

fn leiden(mut graph: Network) -> Vec<Vec<usize>> {
    let m2: i128 = graph.k.iter().sum();
    let n = graph.k.len();
    let mut node: Vec<usize> = (0..n).collect();
    let mut community: Vec<usize> = (0..n).collect();
    // ponytail: no iteration cap, as the spec asks: every iteration that changes something
    // raises the modularity or shrinks the graph, so the loop ends.
    loop {
        let moved = local_moving(&graph, m2, &mut community);
        let (sub, merged) = refine(&graph, m2, &community);
        if !moved && !merged {
            break;
        }
        let mut number: BTreeMap<usize, usize> = BTreeMap::new();
        let mut label: BTreeMap<usize, usize> = BTreeMap::new();
        for &s in &sub {
            let next = number.len();
            number.entry(s).or_insert(next);
        }
        let size = number.len();
        let mut next = Network {
            adj: vec![BTreeMap::new(); size],
            k: vec![0; size],
        };
        let mut start = vec![0; size];
        for v in 0..graph.k.len() {
            let a = number[&sub[v]];
            next.k[a] += graph.k[v];
            let first = label.len();
            start[a] = *label.entry(community[v]).or_insert(first);
            for (&u, &w) in &graph.adj[v] {
                let b = number[&sub[u]];
                if a != b {
                    *next.adj[a].entry(b).or_default() += w;
                }
            }
        }
        for v in &mut node {
            *v = number[&sub[*v]];
        }
        graph = next;
        community = start;
    }
    let mut groups: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for (original, &v) in node.iter().enumerate() {
        groups.entry(community[v]).or_default().push(original);
    }
    let mut groups: Vec<Vec<usize>> = groups.into_values().collect();
    groups.sort();
    groups
}

fn best(candidates: impl Iterator<Item = (usize, i128, usize)>) -> Option<usize> {
    let mut best: Option<(i128, usize, usize)> = None;
    for (label, gain, smallest) in candidates {
        let better = match best {
            None => gain > 0,
            Some((g, s, _)) => gain > g || (gain == g && smallest < s),
        };
        if better {
            best = Some((gain, smallest, label));
        }
    }
    best.map(|(_, _, label)| label)
}

fn local_moving(graph: &Network, m2: i128, community: &mut [usize]) -> bool {
    let n = graph.k.len();
    let mut members: Vec<BTreeSet<usize>> = vec![BTreeSet::new(); n];
    let mut total = vec![0i128; n];
    for v in 0..n {
        members[community[v]].insert(v);
        total[community[v]] += graph.k[v];
    }
    let mut queue: VecDeque<usize> = (0..n).collect();
    let mut queued = vec![true; n];
    let mut moved = false;
    while let Some(i) = queue.pop_front() {
        queued[i] = false;
        let mut weight: BTreeMap<usize, i128> = BTreeMap::new();
        for (&j, &w) in &graph.adj[i] {
            *weight.entry(community[j]).or_default() += w;
        }
        let (own, k) = (community[i], graph.k[i]);
        let stay = m2 * weight.get(&own).copied().unwrap_or(0) - k * (total[own] - k);
        let target = best(weight.iter().filter(|(c, _)| **c != own).map(|(&c, &w)| {
            let gain = m2 * w - k * total[c] - stay;
            (c, gain, *members[c].first().unwrap())
        }));
        let Some(target) = target else { continue };
        members[own].remove(&i);
        total[own] -= k;
        members[target].insert(i);
        total[target] += k;
        community[i] = target;
        moved = true;
        for &j in graph.adj[i].keys() {
            if community[j] != target && !queued[j] {
                queued[j] = true;
                queue.push_back(j);
            }
        }
    }
    moved
}

fn refine(graph: &Network, m2: i128, community: &[usize]) -> (Vec<usize>, bool) {
    let n = graph.k.len();
    let mut in_s = vec![0i128; n];
    for v in 0..n {
        in_s[community[v]] += graph.k[v];
    }
    let mut sub: Vec<usize> = (0..n).collect();
    let mut size = vec![1usize; n];
    let mut total = graph.k.clone();
    let mut smallest: Vec<usize> = (0..n).collect();
    let mut outside: Vec<i128> = (0..n)
        .map(|v| {
            (graph.adj[v].iter())
                .filter(|(u, _)| community[**u] == community[v])
                .map(|(_, w)| w)
                .sum()
        })
        .collect();
    let mut merged = false;
    for v in 0..n {
        if size[v] != 1 || sub[v] != v {
            continue;
        }
        let (s, k) = (in_s[community[v]], graph.k[v]);
        let connected = |t: usize, outside: &[i128], total: &[i128]| {
            m2 * outside[t] >= total[t] * (s - total[t])
        };
        if !connected(v, &outside, &total) {
            continue;
        }
        let mut weight: BTreeMap<usize, i128> = BTreeMap::new();
        for (&u, &w) in &graph.adj[v] {
            if community[u] == community[v] && sub[u] != v {
                *weight.entry(sub[u]).or_default() += w;
            }
        }
        let target = best(
            (weight.iter())
                .filter(|(t, _)| connected(**t, &outside, &total))
                .map(|(&t, &w)| (t, m2 * w - k * total[t], smallest[t])),
        );
        let Some(t) = target else { continue };
        outside[t] += outside[v] - 2 * weight[&t];
        total[t] += k;
        size[t] += 1;
        smallest[t] = smallest[t].min(v);
        sub[v] = t;
        merged = true;
    }
    (sub, merged)
}

#[cfg(test)]
mod tests {
    use super::{Community, communities};

    fn partition(calls: &[(&str, &str)]) -> Vec<Vec<String>> {
        let mut groups: Vec<Vec<String>> = communities(calls.iter().copied(), |_| Some(".".into()))
            .into_iter()
            .map(|c| c.members)
            .collect();
        groups.sort();
        groups
    }

    fn groups(names: &[&[&str]]) -> Vec<Vec<String>> {
        names
            .iter()
            .map(|g| g.iter().map(|s| s.to_string()).collect())
            .collect()
    }

    const DENSE: &[(&str, &str)] = &[
        ("a1", "a2"),
        ("a1", "a3"),
        ("a1", "a4"),
        ("a2", "a3"),
        ("a2", "a4"),
        ("a3", "a4"),
        ("b1", "b2"),
        ("b1", "b3"),
        ("b1", "b4"),
        ("b2", "b3"),
        ("b2", "b4"),
        ("b3", "b4"),
        ("a4", "b1"),
    ];

    #[test]
    fn two_dense_groups_joined_by_one_edge_give_two_communities() {
        assert_eq!(
            partition(DENSE),
            groups(&[&["a1", "a2", "a3", "a4"], &["b1", "b2", "b3", "b4"]])
        );
    }

    #[test]
    fn a_relabelled_copy_gives_the_same_partition() {
        let rename = |s: &str| {
            let (group, n) = s.split_at(1);
            let group = if group == "a" { "y" } else { "x" };
            format!("{group}{}", 5 - n.parse::<u8>().unwrap())
        };
        let renamed: Vec<(String, String)> = DENSE
            .iter()
            .rev()
            .map(|(a, b)| (rename(b), rename(a)))
            .collect();
        let renamed: Vec<(&str, &str)> = renamed
            .iter()
            .map(|(a, b)| (a.as_str(), b.as_str()))
            .collect();
        let mut back: Vec<Vec<String>> = partition(&renamed)
            .into_iter()
            .map(|g| {
                let mut g: Vec<String> = DENSE
                    .iter()
                    .flat_map(|(a, b)| [*a, *b])
                    .filter(|s| g.contains(&rename(s)))
                    .map(String::from)
                    .collect();
                g.sort();
                g.dedup();
                g
            })
            .collect();
        back.sort();
        assert_eq!(back, partition(DENSE));
    }

    #[test]
    fn ties_go_to_the_community_with_the_lowest_smallest_member() {
        let cycle = [("a", "b"), ("b", "c"), ("c", "d"), ("d", "a")];
        assert_eq!(partition(&cycle), groups(&[&["a", "b"], &["c", "d"]]));
    }

    #[test]
    fn a_symbol_with_no_edge_is_a_singleton_and_a_file_is_no_node() {
        assert_eq!(partition(&[("a", "a")]), groups(&[&["a"]]));
        let dir = |s: &str| (s != "f.ts").then(|| ".".to_string());
        let calls = [("f.ts", "b"), ("f.ts", "f.ts"), ("c", "d")];
        let found: Vec<Vec<String>> = communities(calls.iter().copied(), dir)
            .into_iter()
            .map(|c| c.members)
            .collect();
        assert_eq!(found, groups(&[&["b"], &["c", "d"]]));
    }

    #[test]
    fn call_sites_weigh_and_calls_to_self_are_no_edges() {
        let path = [("a", "b"), ("b", "c"), ("b", "c"), ("c", "b"), ("c", "c")];
        assert_eq!(partition(&path), groups(&[&["a", "b", "c"]]));
    }

    #[test]
    fn a_numbered_name_skips_a_directory_of_that_name() {
        let dir = |s: &str| match s {
            "x" | "y" => Some("lib#2".to_string()),
            _ => Some("lib".to_string()),
        };
        let calls = [("lib/a", "lib/b"), ("lib/c", "lib/d"), ("x", "y")];
        let names: Vec<String> = (communities(calls.iter().copied(), dir).into_iter())
            .map(|c| format!("{} {}", c.name, c.members.join(" ")))
            .collect();
        assert_eq!(names, ["lib lib/a lib/b", "lib#2 x y", "lib#3 lib/c lib/d"]);
    }

    #[test]
    fn names_are_the_most_common_directory_with_collisions_numbered() {
        let dir = |s: &str| Some(s.rsplit_once('/').map_or(".", |(d, _)| d).to_string());
        let calls = [
            ("src/a", "src/b"),
            ("src/b", "root"),
            ("src/a", "root"),
            ("src/x", "src/y"),
            ("lib/p", "app/q"),
        ];
        let community = |name: &str, members: &[&str]| {
            let mut community = Community::default();
            community.name = name.into();
            community.members = members.iter().map(|m| m.to_string()).collect();
            community
        };
        assert_eq!(
            communities(calls.iter().copied(), dir),
            vec![
                community("app", &["app/q", "lib/p"]),
                community("src", &["root", "src/a", "src/b"]),
                community("src#2", &["src/x", "src/y"]),
            ]
        );
    }
}
