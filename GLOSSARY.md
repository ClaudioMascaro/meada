# Meada

Meada indexes repositories, links them through the contracts they share, and answers questions
about the resulting map. This glossary names the concepts the docs, the code and the issues use.

## Language

### Code and index

**Repository**:
A git repository that Meada indexes on its own. It is the unit of ownership and of the index.
_Avoid_: project, codebase, service

**Index**:
What Meada builds for one repository: its symbols, calls and contracts, with no source text.
_Avoid_: database, cache

**Span**:
A range in a source file, stored as a location and a content hash. Its text is read from disk
when shown.
_Avoid_: snippet, line range

**Language pack**:
What Meada knows about one language: the file extensions it handles and the tree-sitter
queries that read what one file defines, imports, exports and calls. It never touches the index
or other files. A pack is done when it passes the conformance suite over its fixture.
_Avoid_: plugin, parser, grammar

**Language adapter**:
The seam between a language pack and Meada's shared scope and import resolution, which
connects each call to the definitions it reaches and gives the call a level.
_Avoid_: driver, frontend, bridge

### Across repositories

**Peer**:
A repository that another repository's `meada.toml` names as one it shares contracts with.
_Avoid_: member, group, neighbour, dependency

**Workspace**:
The repositories one query can see: the repository where Meada runs, its peers through their
`meada.toml` files (transitively), and the sibling clones whose `meada.toml` names a repository
already in it. Each keeps its own index.
_Avoid_: group, monorepo, organization

**Provider**:
The side of a contract that serves or defines it, such as the code handling a route or the
manifest of a package.
_Avoid_: server, backend

**Consumer**:
The side of a contract that calls or uses it, such as a client call to a route or a dependent of
a package.
_Avoid_: client, caller

**Unresolved consumer**:
A consumer whose contract Meada could not read from the code after folding constants and
client wrapper arguments, such as a `fetch` of a URL computed at run time. It is shown in
search, context and the export, never linked.
_Avoid_: dynamic call, unknown call

**Client wrapper**:
A function whose call to an HTTP client (`fetch` in v0.1) takes its URL or method from the
function's own parameters. Each call site of the wrapper is a consumer, read with its
arguments in place of the parameters, through any number of wrapper levels.
_Avoid_: API client, helper, fetcher

**Contract**:
What one repository exposes and another depends on, such as an HTTP route, a queue, a package or
a shared resource.
_Avoid_: interface, integration

**Contract kind**:
A category of contract with its own matching rules, such as HTTP route, queue or package.
_Avoid_: contract type, protocol

**Resource**:
A shared thing that repositories read or write, such as a database, a bucket or a content
dataset, declared in `meada.toml`.
_Avoid_: datastore

**Link**:
A connection Meada makes between a consumer and a provider through a contract. It carries a
level and a reason.
_Avoid_: edge, bridge, connection

**Level**:
How well a link is established: exact, inferred, declared, decided or ambiguous. It is a name,
not a score.
_Avoid_: confidence, score, weight

**Chain**:
A sequence of links followed across repositories, such as A → B → C. It is as strong as its
weakest link.
_Avoid_: path, flow

**Hop**:
One link in a chain.
_Avoid_: step, jump

### Decisions

**Open item**:
An ambiguous link or an unresolved consumer (a dynamic URL) of a repository that no decision
resolves. Its id is the call-site identity, and its candidates are the providers the link lists
or the routes the call's method and evaluated URL fragments leave. `search` lists open items
and `meada decide` resolves them.
_Avoid_: pending item, todo

**Decision**:
A recorded resolution of an ambiguous link or a dynamic URL, kept in `meada.decisions.toml` with
who made it.
_Avoid_: override, verdict

**Decider**:
Whoever makes a decision: a person or, if the user opts in, a language model reached through
`meada decide`.
_Avoid_: judge, LLM, oracle

### Testing

**Golden file**:
A checked-in file holding the expected output of the query engine over a fixture, such as an
export or a rendered answer. Tests compare against it; it changes only through the explicit
bless mode (`MEADA_BLESS=1`) and is reviewed as a diff.
_Avoid_: snapshot, expected output

### Roadmap

**Track**:
A line of roadmap work that can run beside the others: A (differentiators), B (languages) or C
(communities and processes).
_Avoid_: stream, epic, milestone
