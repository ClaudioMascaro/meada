# 22. Authenticated workspace proposals over MCP

- Status: accepted
- Date: 2026-10-30

## Context

Spec S5 (#339) wants a CLI agent to help an operator author a named workspace: the agent reads
repositories with its own GitHub read tools, and Meada tells it which repositories the operator
may select and whether a proposed manifest is valid and authorized. The agent hands the result to
a person, who saves and applies it in the console.

Three existing decisions bound the design:

- ADR 0002, invariant 4: the MCP server has no tool that writes; a write tool needs a new ADR.
  The nodes' query server (ADR 0008, ADR 0011) is read-only and its tools, descriptions and
  goldens do not change.
- ADR 0014: the console is never a route of a node and never an MCP server, and it answers
  browsers only, behind `Host` and `Origin` checks.
- ADR 0012 and ADR 0016: no token passthrough, no outbound call from a console or node to GitHub
  or the identity provider, secrets only from the environment or files, and durable state only in
  the Postgres schema.

ADR 0020 and ADR 0021 settle who may create and view a workspace and what the manifest, the
catalog and the version lifecycle are. This ADR reviews the one new surface the spec adds: its
transport, audience, admission, inputs, outputs and statelessness. Its tickets are #346 (the
interface) and #347 (acceptance).

## Decision

### A process of its own

- `meada workspaces mcp --listen <addr> --workspace <bootstrap manifest>` runs the **proposal
  server**. It is a cloud module in ADR 0012's shape: the cargo feature `proposals`, off by
  default, implying `workspaces`, enabled by `cloud`, plus a `[proposals]` section (`public_url`,
  required, `https` unless loopback; `audience`, `public_url` by default) that needs
  `[workspaces]`. The subcommand is absent from a build without the feature, so the default CLI
  help, the node's MCP surface and their goldens do not change.
- It is not a node and not the console: its own listener, port and pod, the hyper listener `meada
  mcp --listen` uses, Streamable HTTP and stateless as the nodes are (no `Mcp-Session-Id`, no
  sessions), `Origin` refused with 403 before anything else, no cookie and no `Access-Control-*`
  header. The nodes keep exactly their tools; the console keeps no MCP surface.
- It connects to what the bootstrap manifest configures and nothing else: Postgres (permissions
  sets, operators, workspaces, the directory), the snapshot store (version documents), Valkey and
  the generations it holds for the effective configuration, and the issuer's metadata and JWKS.
  It never calls GitHub, a node or the identity provider's API, and holds no GitHub App key.

### Authentication and admission

- Admission is the nodes' OIDC module with its own audience: a bearer JWT whose `aud` is
  `[proposals] audience` (RFC 8707, the resource's canonical URL), with protected resource
  metadata (RFC 9728) served without a token. A token for a node or for the console is not
  accepted here and this server's token is accepted nowhere else, which closes the confused
  deputy. A missing or invalid token is a 401 before any routing or tool name is resolved.
- Meada derives, server-side and from this token only, the subject, its login (`login_claim`),
  its groups, its grant, its role, the workspaces it operates and its eligibility (ADR 0020).
  A subject without a login gets `pending` with `missing_login`; a subject the directory
  deprovisioned is refused, since this server reads Postgres as the console does (a node's
  statelessness does not apply). A deleted user stays blocked until an explicit active
  reprovisioning, as in the console.
- No tool has a parameter naming an actor, owner, role, grant, login, group, token or
  permission. Every tool's input schema is closed (`additionalProperties: false`) and a
  property that is not declared is refused (`invalid_params`), not ignored, so a forged
  assertion cannot be mistaken for accepted input. Request metadata (`_meta`, headers other than
  `Authorization`) decides nothing.
- The agent's GitHub credentials stay with its GitHub connector. No tool accepts a credential,
  Meada receives none and forwards none, and the `Authorization` header it does receive is
  Meada's own token, never passed on.

### Tools

Four tools, all `readOnlyHint: true`, `destructiveHint: false`, `idempotentHint: true`,
`openWorldHint: false`. They are the console's functions, called with persistence off.

| Tool | Input | Output |
| --- | --- | --- |
| `catalog` | `query?` (at most 128 characters), `limit?` (1 to 100, default 50), `after?` (cursor) | ADR 0021's catalog answer: state, reason, set freshness, the identities the subject can select, `next` |
| `workspaces` | `limit?` (1 to 100, default 25), `after?` | the workspaces the subject may see: `id`, `name`, `seq`, `version`, `state`, and whether it operates each |
| `show` | `workspace` (an id) | one visible workspace: `id`, `name`, `description`, `seq`, `version`, `state`, the canonical manifest text of the entries the subject reads, the hidden-entry count for operators; `too_large` instead of a truncated answer above the output cap |
| `propose` | `entries` (manifest text, at most 64 KiB), `workspace?` (an id: the base), `name?`, `description?`, `hints?` (at most 50 `{identity, note?}`, `note` at most 256 characters) | the proposal, below |

- **A proposal** is `{base: {workspace, seq, version} or null, manifest, diff, diagnostics,
  eligibility, refusals, effective, applicable}`: `manifest` is the canonical text the console
  would store (`entries_text` over the validated entries, no view annotation in it); `diff` is
  `meada-core`'s diff against the base's readable entries, or null for a new workspace;
  `diagnostics` are the validation problems with the CLI's English text and their entry and key;
  `eligibility` is ADR 0021's per-identity state for the entries and hints; `refusals` are the
  authorization refusals the console would give an apply (`entries_refused` shape, naming only
  identities its grant reads and counting the rest in `hidden`; an identity the caller submitted
  but cannot read appears only as `unavailable` in `eligibility`, a state that is uniform);
  `effective` is the effective-configuration view when the base exists; `applicable` says
  whether the console would accept it now. `base` is the fact the console's `base` must still
  find unchanged.
- **Hints are hints.** `hints` and their notes never add an entry and never change a result:
  inclusion is decided by the identities in `entries` against the verified catalog and the
  subject's roles. A note is bounded and discarded, not stored, logged or echoed. Repository
  metadata an agent read elsewhere (descriptions, languages, claims about access) is untrusted
  text with no effect on any decision.
- **Parity.** The tools call the same `meada-core` functions as the console (named-entry
  validation, diff, composition, eligibility, effective configuration) through the same server
  module that serves `workspaces/` routes, with persistence off. The shared fixture table of ADR
  0020 is the test for both.

### Statelessness

- A tool writes nothing: no Postgres row, no store object, no Valkey key, no local file, no
  draft, no version, no moved pointer, no audit row. Its output depends only on its input and
  the state read: no randomness or counter reaches it, and the only time it shows is the
  permissions set's age, which is state aging, so equal calls on equal state give equal bytes
  apart from `age_seconds`.
- The only trace of a call is one stderr line, `proposals: <subject> <tool> <workspace or -> :
  <outcome>`, without a token, a manifest, a hint or an identity outside the grant.
- Saving and applying stay with a person in the console: it pastes or imports the `manifest`,
  validates it again, and its save and apply recheck the base, the authority and the current
  eligibility (ADR 0021). The console trusts nothing in a proposal: a forged one is only text
  that validates or does not. No deep link or URL carries a manifest.

### Bounds

- Request bodies are capped at 80 KiB (413 beyond; a manifest is at most 64 KiB, ADR 0021);
  every list is paged by the limits above; a tool's output is capped at 192 KiB (a manifest, its
  diff and diagnostics) and is never truncated silently (`too_large`, with counts).
  Arrays and strings have the lengths above; every identity is checked against
  `identity_problem` and every id against its pattern before any query. SQL is parameterized.
- A call is bounded by `[postgres] statement_timeout_seconds` and the server by a limit of 32
  calls in flight (a further call is 503 `busy`). There is no per-subject quota; the audience
  and the identity provider's own token lifetime are the abuse bound, and nothing a tool returns
  can be turned into a persisted effect.
- No tool answer carries a repository the subject cannot select or read, or a workspace it
  cannot see. An error for an unknown, deleted or hidden workspace is one identical
  `not_found`.

### Review against invariant 4

ADR 0002, invariant 4 reads "the MCP server has no tool that writes: it never changes the user's
files, configuration or decisions". None of the four tools changes a file, configuration,
decision or any state of Meada. They return derived text. The invariant holds in its letter and
its intent, so ADR 0002 needs no amendment beyond a pointer here. The reasons it holds are
structural, not a promise: the module has no store, Valkey or Postgres write path compiled into
its tool handlers (the shared authoring module takes a read-only handle in this process); the
node query server gains no tool; and the default build has neither the feature nor the
subcommand.

A later tool that saves, applies, comments or otherwise persists is a write tool and needs its
own ADR, which must decide: the write capability and its name, an audience or scope distinct
from this read-only one, human confirmation, ADR 0016's fail-closed audit record per call, and
how a stale base is refused. Automatic save or apply by an agent is out of scope for S5.

## Considered options

- **Tools on the console's listener.** One process fewer, and it makes the console an MCP server
  (ADR 0014 forbids it), mixes browser admission (`Origin` required for writes) with agent
  admission (`Origin` refused) and gives agent traffic a path to the console's write routes.
- **Tools on the nodes.** The nodes would need Postgres for operators and the permissions set on
  the request path (ADR 0016 keeps them stateless) and would change the query MCP's tool list.
- **Persisting drafts for the console to open.** A durable submission is a write tool; it needs
  fail-closed audit and a separate decision. Copying text is enough for the first interface.
- **Trusting the agent's reported permissions, or forwarding its GitHub token so Meada could
  verify them.** The first lets any agent assert a grant; the second is token passthrough (ADR
  0012) and a GitHub call from a process that must make none. Roles come from the job's
  permissions set.
- **Sessions or streaming.** Nothing the tools return needs state, and ADR 0012 closed sessions.
- **A `grant` or `actor` argument "for convenience".** An argument that names authority is an
  invitation to forge it. There is none.

## Consequences

- Invariants of ADR 0002: 1 (no network or model in indexing or validation: the core rules are
  deterministic and no model runs), 2 (the server connects only inside the boundary the
  manifest configures), 3 (it writes nothing), 4 (reviewed above), 5 and 6 (unchanged; outputs
  are deterministic).
- ADR 0008 is unchanged: the nodes' endpoints, tools and answers do not move.
- The operator owns one more process, its audience in the identity provider and its URL. An
  agent client needs a token for that audience; how it obtains one is the identity provider's.
- #346 proves, at the public binary with the loopback issuer, fake GitHub REST and local
  services: forged actor, role and grant arguments refused; a token for another audience
  refused; no outbound call and no credential forwarded; the Y/A/B table through the tools equals
  the console's answers; hidden and unauthorized repositories counted, never named; oversized
  inputs and outputs bounded; and equal calls equal bytes (`age_seconds` aside) with Postgres, the
  store and Valkey unchanged afterwards.
- #347 documents how an agent uses its own GitHub read MCP beside this server and hands the
  proposal to the console, in three languages.
