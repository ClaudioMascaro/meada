# 5. Cross-repo model

- Status: accepted
- Date: 2026-10-14

## Context

Cross-repo context is Meada's headline feature. Prior art matches contracts by normalized
identifier and follows one hop. It strips leading host templates but not literal prefixes, API
versions, base URLs read from the environment or server mount prefixes. Its cross-repo
configuration is global or absent, so an agent cannot find it in the repository. This ADR fixes
what Meada links, how repositories declare their relations, and how a link states its own
strength.

## Decision

### Contract kinds

Through v1, Meada links these kinds of contract:

- HTTP route and its client call (v0.1).
- Declared shared resources, such as a database, a bucket or a content dataset.
- OpenAPI and AsyncAPI documents, as a provider source, including shapes (ADR 0009).
- Queues and topics, matched by literal name.
- Packages. The manifest defines the package and a dependent gets a link. By v1, an import
  resolves to the exported symbol.

Environment variables matched by name are out. Their values are secrets that Meada never reads,
and the same value often sits under different names in each repository.

### Configuration

A `meada.toml` at each repository root declares what that side owns. It is versioned with the
code.

- The consumer lists `[[peer]]` entries: `repo` (the peer's git remote identity), an optional
  `path` hint and `base_url_env`. Meada never reads the value of that variable. The name tells
  it which peer a call built from that variable goes to.
- The provider declares `[serve]`: `hosts`, plus the gateway `prefix` and `strip` rewrites.
- Either side lists `[[resource]]` entries: the kind, an id and a role (read or write).

### Discovery

From the repository where the command runs, Meada finds peers by their `path` hint, or by
reading the `.git/config` remotes of neighbouring clones. It uses no network (ADR 0002,
invariant 4). Chains are transitive through each peer's own `meada.toml`.

### Prefixes

- Mount prefixes are read from the code: Express `app.use('/api', router)`, FastAPI
  `include_router(prefix=...)`, a class-level `@RequestMapping`, and their equivalents.
- Gateway prefixes and rewrites are declared in `[serve]`.
- API versions are never stripped automatically. `/v1` and `/v2` are different contracts.

### Levels and chains

Each link carries a level and a reason. The levels are:

- **exact**: both sides match as written.
- **inferred**: they match after Meada resolved a base URL or a prefix.
- **declared**: the link comes from `meada.toml`.
- **decided**: the link comes from `meada.decisions.toml` (ADR 0006).
- **ambiguous**: several candidates fit. They are listed, never silently picked.

A chain is as strong as its weakest link. There are no tuned weights. The levels compare in
this order: exact > declared > inferred > decided > ambiguous. `decided` sits below `inferred`
because it covers exactly what mechanical resolution could not, and may be a model's judgment.
(Amended 2026-10-14; the first version left the order to the v0.1 spec.)

A chain has no hop limit. Meada follows it until it ends or the token budget runs out, and it
detects cycles.

## Considered options

- **A central group repository.** One more repository, plus a pointer in every repository.
- **Configuration inside `.meada/`.** Less visible, and it needs an exception to the
  self-ignoring `.gitignore`.
- **A global configuration outside version control.** What prior art does. It fails the aim of
  keeping cross-repo configuration in version control.
- **Automatic prefix and version heuristics.** They produce false positives across versions.
- **A numeric 0 to 1 score.** It needs tuned weights and gives false precision.
- **One hop (prior art) or a fixed N hops.** Real systems chain deeper, and any fixed N is
  arbitrary.
