# Per-key configuration merge

Meada does not merge a repository's own `meada.toml` with the blocks a manifest entry or a named
workspace's context declares for it, key by key or any other way. When the repository has its own
file, that file wins whole and the other source's blocks are ignored and reported: `meada status`
names them for the deployment manifest, and a named workspace's effective-configuration view marks
them `ignored_by_repository`. When the repository has no file, the context's blocks apply whole.

## Why this is out of scope

ADR 0011 rejected "manifest entries merged field by field with `meada.toml`": two sources for one
fact, and the repository's owners cannot tell from their file what the server sees. ADR 0021
(Contract 6) keeps the rule for every context: a repository's configuration is decided by layers
with provenance, never merged.

A per-key merge would let a context extend one key of a repository's declarations, say one more
`serve` host, without touching the repository. The price is that "what does the server use for
this repository?" would depend on the file plus every context's blocks, and the owners could no
longer answer it from their own file. The effective-configuration view would have to show a merge
result with provenance per key, and two contexts extending the same key would need an order
between them.

What works today: the repository's owners change its `meada.toml` (a reviewed change in that
repository), or the repository keeps no file and each context's blocks apply whole, each context
seeing only its own. The effective-configuration view says which source won before a save is taken
as meaningful.

Reopening this starts with a new ADR that supersedes ADR 0011's rejection, from a team's concrete
case that neither of those covers.

## Prior requests

- #509: "Named workspaces: per-key merge of `meada.toml` with a context's blocks, and a
  directory-backed operator picker" (the merge half; the picker half was deferred under ADR 0021's
  "Open", not rejected)
