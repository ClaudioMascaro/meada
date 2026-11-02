# Registry metadata

Meada 1.x does not read package registry metadata: no published versions, licences, security
advisories or maintenance signals from crates.io, npm, PyPI, the Go module proxy, Maven Central,
NuGet or any other registry, neither during indexing nor in the MCP server.

## Why this is out of scope

ADR 0002, invariant 1, says indexing never calls the network. Registry metadata only exists
behind a network call, so reading it during `meada index` would break the invariant outright.
Reading it at query time instead would make every answer depend on a registry's availability and
rate limits, the same objection `server-side-decider.md` records for a model.

Invariant 6 is the deeper problem. The same inputs (the source tree, `meada.toml` and
`meada.decisions.toml`) give the same answers and the same export. A registry answers differently
from one day to the next, so a field read from it makes the export depend on when it was built.
A local cache the index reads would have to become a fourth input, committed or recorded in the
export, to keep the rule.

What the repository already holds stays in scope: manifests today, and lockfiles next (#537), give
the declared range and the version that resolves without leaving the machine.

## What a future change would need

A new ADR. The shape #537 sketched is an opt-in command that fetches into a local file the index
then reads, the way `meada decide` is the opt-in exception for a model (ADR 0006). That ADR would
have to say how the file enters invariant 6's inputs, who refreshes it, and what an answer says
when it is stale or missing.

## Prior requests

- #537: "Read more dependency sources: lockfiles, CODEOWNERS and registry metadata" (the registry
  part only; lockfiles and CODEOWNERS stay in scope)
