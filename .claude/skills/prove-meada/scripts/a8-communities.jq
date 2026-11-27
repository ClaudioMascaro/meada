# Communities of a workspace whose calls the specified resolutions change (#973's Go `type` rows,
# while the base predates them): deterministic Leiden over those calls (spec #31,
# `src/community.rs`, unchanged since 1.5.0) gives groups no spec line lists, so they are checked
# by what spec #31 states instead, against the expected calls (`a8-expected.jq`), never the head's
# own calls. Weaker than byte equality: a regrouping that keeps every group connected passes.
# Input: the head export; `$expected[0]`: the expected export. Output: one line per failure.
def dir: split("::")[1] | split("/") | if length > 1 then .[:-1] | join("/") else "." end;
# The most common parent directory of the members, ties by lexical order (`communities`).
def natural: map(dir) | group_by(.) | map({d: .[0], n: length})
  | (map(.n) | max) as $most | map(select(.n == $most)) | sort_by(.d) | .[0].d;
($expected[0]) as $e
| ([$e.symbols[].id] | INDEX(.)) as $symbols
# A node per symbol of an exact or inferred call; the holder stands for a top-level caller
# (`write_communities`, `COALESCE(holder, caller)`).
| [$e.calls[] | select((.level == "exact" or .level == "inferred") and .target != null)
    | [(.holder // .caller), .target] | map(select($symbols[.] != null))] as $pairs
| ([$pairs[][]] | unique) as $nodes
| [$pairs[] | select(length == 2 and .[0] != .[1])] as $edges
| [.communities[].members[]] as $members
| def connected($group):
    ($group | INDEX(.)) as $in
    | [$edges[] | select($in[.[0]] != null and $in[.[1]] != null)] as $inner
    | {seen: [$group[0]], todo: [$group[0]]}
    | until(.todo == [];
        .todo[0] as $n
        | (([$inner[] | select(.[0] == $n) | .[1]] + [$inner[] | select(.[1] == $n) | .[0]]) | unique) as $next
        | ($next - .seen) as $new
        | .seen += $new | .todo = .todo[1:] + $new)
    | (.seen | sort) == ($group | sort);
  # Names in order of each group's smallest member: the natural name, or on a collision the
  # next `#n` from 2 that no community took and no community is naturally named.
  ([.communities[] | {members: (.members | sort)} | .natural = (.members | natural)]
    | sort_by(.members[0])
    | ([.[].natural] | unique) as $reserved
    | reduce .[] as $c ({taken: [], names: {}};
        (if any(.taken[]; . == $c.natural) | not then $c.natural
         else . as $state
           | first(range(2; infinite) | "\($c.natural)#\(.)"
               | select(. as $n | (any($state.taken[]; . == $n) or any($reserved[]; . == $n)) | not))
         end) as $name
        | .taken += [$name] | .names[$c.members[0]] = $name)
    | .names) as $names
  | (if ($members | sort) != $nodes then
       "members differ from the symbols of exact and inferred calls: missing \($nodes - $members), extra \($members - $nodes)"
     else empty end),
    (if ($members | length) != ($members | unique | length) then "a symbol is in two communities" else empty end),
    (.communities[]
      | .name as $name
      | $names[.members | sort | .[0]] as $want
      | (if $name != $want then "\($name): spec #31 names it \($want)" else empty end),
        (if connected(.members) then empty else "\($name): members not connected by the expected calls" end))
