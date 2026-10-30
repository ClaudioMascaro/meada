# Decidable package kind

Meada does not offer ambiguous package links to `meada decide`. A dependency entry that several
providers fit links to each of them at the level `ambiguous`, and so does a call through an import
of it; neither becomes an open item, so `meada decide` never asks about them. The open items stay
what spec A5 (#22) and ADR 0006 set for v1: ambiguous links of the other kinds and dynamic URLs.

## Why this is out of scope

No real workspace needs it yet. An export (`meada export`) of the maintainer's dogfood set on
2026-10-30, 13 local repositories, found no ambiguous package link:

- A package link needs a dependency entry that names a package of the workspace. Besides Meada
  itself, only one of the 13 repositories has such entries (a Cargo workspace of 7 crates), and
  its 7 package links are all `exact`.
- Three of them, once their peers were declared, form a three-repository workspace. It links over
  HTTP only (4 cross-repository links), and none of its repositories names another's package.
- Every ambiguous package link in Meada's own repository (27) comes from `tests/fixtures/`, forks
  made on purpose.

And the work is larger than making the kind decidable. Replay does not ask whether a kind is
decidable: a decision written by hand on a dependency entry whose providers sit in different
repositories already applies, but only to the entry's own link. Beyond that case:

- A decision names its target as `<provider repository identity>::<contract identifier>`, with no
  site, in a file whose keys `docs/site/en/stability.md` lists as frozen for 1.x. Two providers of
  one package in the same repository (a fork kept beside the original in a monorepo) cannot be
  told apart by it: replay would take the first in order, whichever the person meant. Neither can
  an ambiguity inside one provider (no way in leads to a file, and two files export the imported
  name). Deciding those needs a new key in `meada.decisions.toml`.
- A call through a package import links to the symbol it reaches in each provider, while the
  package's providers are its manifests. Replay accepts a candidate only when the link's provider
  is the site of a provider serving the contract, so a decision on such a call would always be
  stale with `not-candidate`.
- A decided dependency entry would have to carry its choice to the calls through its imports,
  which replay would re-filter by the chosen provider: a rule it does not have.

What already exists: a dependency entry is a consumer with a stable identity
(`<repository>::<manifest>::<field>::<name>`), and a decision's `call` accepts a consumer site, so
the entry needs no new identity (ADR 0004). Even so, the change is a spec of several tickets (an
amendment to ADR 0006, a decision rule per link rather than per kind, the decisions file key, two
replay rules, the `meada decide` prompt and its excerpt) for a case no real workspace shows.

What works today: an `ambiguous` package link names every fitting provider with its reason, so a
reader or an agent sees the candidates. Keeping one provider of a name in the workspace (renaming
a fork's package, or leaving the fork out of the peers) makes the link `exact`.

## Reopening

Reopen from a real workspace, not a fixture, where a dependency entry fits several providers and
the workspace cannot keep a single provider of the name. Attach the `meada export` link lines of
the entry and of the calls through it. The design above (the target key, the replay rules and the
ADR 0006 amendment) starts from that case.

## Prior requests

- #392: "Package links as open items for `meada decide` (after R1)", split from #352 during triage
