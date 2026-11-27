# Communities of a workspace whose calls the specified resolutions change (#973's Go `type` rows):
# deterministic Leiden over those calls (spec #31, `src/community.rs`, unchanged since 1.5.0)
# gives groups no spec line lists, so they are checked by what spec #31 guarantees instead, against
# the expected calls (`a8-expected.jq`), never the head's own calls. Input: the head export;
# `$expected[0]`: the expected export. Output: one line per failure, none when every check holds.
def dir: split("::")[1] | split("/") | if length > 1 then .[:-1] | join("/") else "." end;
($expected[0]) as $e
| ([$e.symbols[].id] | INDEX(.)) as $symbols
| [$e.calls[] | select(.level == "exact" or .level == "inferred") | [.caller, .target]
    | map(select($symbols[.] != null))] as $pairs
| ([$pairs[][]] | unique) as $nodes
| [$pairs[] | select(length == 2 and .[0] != .[1])] as $edges
| [.communities[].members[]] as $members
| def connected($group):
    ($group | INDEX(.)) as $in
    | [$edges[] | select($in[.[0]] != null and $in[.[1]] != null)] as $inner
    | {seen: [$group[0]], todo: [$group[0]]}
    | until(.todo == [];
        .todo[0] as $n
        | ([$inner[] | select(.[0] == $n) | .[1]] + [$inner[] | select(.[1] == $n) | .[0]]) as $next
        | ($next - .seen) as $new
        | .seen += $new | .todo = .todo[1:] + $new)
    | (.seen | sort) == ($group | sort);
  (if ($members | sort) != $nodes then
     "members differ from the symbols of exact and inferred calls: missing \($nodes - $members), extra \($members - $nodes)"
   else empty end),
  (if ($members | length) != ($members | unique | length) then "a symbol is in two communities" else empty end),
  (if ([.communities[].name] | length) != ([.communities[].name] | unique | length) then "two communities share a name" else empty end),
  (.communities[]
    | .name as $name
    | (.members | map(dir) | group_by(.) | map({d: .[0], n: length})
        | (map(.n) | max) as $most | map(select(.n == $most)) | sort_by(.d) | .[0].d) as $natural
    | (if $name != $natural and ($name | startswith($natural + "#") | not)
       then "\($name): not named after its most common directory \($natural)" else empty end),
      (if connected(.members) then empty else "\($name): members not connected by the expected calls" end))
