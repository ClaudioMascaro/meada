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
queries that read what one file defines, imports, exports and calls, and the producer and
consumer calls of the message libraries it knows. It never touches the index. A pack is done when it passes the conformance suite over its fixture.
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
A function whose call to an HTTP client (`fetch`, or `requests`, `httpx` or `aiohttp` in
Python) or a message client (a Kafka producer) takes its URL, method or channel name from the
function's own parameters. Each call site of the wrapper is a consumer or producer, read with its
arguments in place of the parameters, through any number of wrapper levels.
_Avoid_: API client, helper, fetcher

**Contract**:
What one repository exposes and another depends on, such as an HTTP route, a channel, a package
or a shared resource.
_Avoid_: interface, integration

**Contract kind**:
A category of contract with its own matching rules, such as HTTP route, channel or package.
_Avoid_: contract type, protocol

**Gateway prefix**:
The rewrite a gateway applies to the paths of a repository: `prefix` is added in front, `strip`
is removed from the start. It is declared in `[serve]`, and the **server path** of each root
`servers` URL of a provider document (`/gw` in `https://api.example.com/gw`) is a prefix too,
for that document's routes, with the `[serve]` strip. A route is also matched by its gateway
path (the prefix followed by its code path without the stripped part); a link made that way is
inferred and its reason names the rewrite. Within one repository a code path match wins.
_Avoid_: base path, path rewrite

**Provider document**:
An OpenAPI or AsyncAPI file of a repository with a `[serve]` table, found by its name or listed
in `[serve] documents`. Its routes and channel sides are provider contracts of the repository,
each merged into the code side with the same contract when there is one; what it states of a
payload is the side's shape.
_Avoid_: spec file, schema file, contract file

**Documented only**:
A route or channel side that only a provider document lists, with no handler or producing call
in the code. It shows as `<document> (documented only)` in search, context and trace, and a
trace does not enter it.
_Avoid_: virtual route, phantom side

**Shape**:
What a contract carries in its payload, recorded as a set of field paths: dotted, with array
elements written `[]` (`items[].id`), and `[]` first for a payload that is itself an array
(`[].id`). What is recorded is presence, the fields known to be scalars and, for a consumer,
the fields its type marks optional; types are never compared. A part of a shape can be **open**, meaning
Meada cannot list its fields, such as an object that allows any extra property or a schema
behind an external reference; anything below an open part may be present. A shape names its
source, such as the provider document it was read from, and has one form whatever that source
is.
_Avoid_: schema, payload type, body

**Drift**:
A field path in a consumer's shape that the shape of the provider it links to does not contain:
a field the consumer reads that the provider no longer returns. Paths are compared from the
root; one that reaches an open part of the provider's shape is not checked and never drift.
Each drifting path carries its own level, `exact` when the provider's shape and the consumer's
path are both declared (a document, a schema, a type) and `inferred` when either comes from
what code does (field reads, returned literals), while the link keeps its level. `trace`,
`context`, `search` and `meada status` show it.
_Avoid_: breaking change, mismatch, schema diff

**Resource**:
A shared thing that repositories read or write, such as a database, a bucket or a content
dataset, declared in `meada.toml`.
_Avoid_: datastore

**Writer** and **reader**:
The sides of a resource: what writes it (a provider of its contract) and what reads it (a
consumer). Each is a `via` symbol of the entry, or the repository as a whole.
_Avoid_: producer, subscriber

**Channel**:
A queue or topic of a broker, identified by its broker kind and its name as evaluated:
`channel::kafka::orders.created`. The same name under two broker kinds is two channels; broker
clusters are not modelled, so one name of one kind is one channel across the workspace. Every
consumer of a channel links to every producer of it, in the same repository too. A Redis stream
is a channel of the broker kind `redis`.
_Avoid_: queue (for the kind), topic (for the kind), stream (for the kind)

**Producer**:
The provider side of a channel: the call that sends to it, or the call site of a client wrapper
that leads to one. Its consumer side is the handler of the messages, the function a subscription
names. Messages go from producer to handler, so `trace` goes down that way. A RabbitMQ publish
to a named exchange is a producer of no channel by itself: it produces to each queue a binding
routes it to.
_Avoid_: publisher, sender, writer

**Unresolved side**:
A producer or consumer of a channel whose name Meada could not evaluate, such as a topic read
from the environment. Its contract names only the broker kind (`channel::kafka`), it shows as
`(unresolved)` in search, context, trace and the export, and it is never linked nor an open item.
_Avoid_: hole, dynamic channel

**Binding**:
A RabbitMQ `bindQueue(queue, exchange, pattern)`: neither side of a channel, but the rule that
sends a publish to an exchange to a queue whose pattern matches the routing key. Bindings from
any repository of the workspace apply, whatever the exchange type. The match is `exact` when the
pattern is the key and `inferred` for a wildcard.
_Avoid_: route, subscription

**Repository-level**:
A resource side that is the repository as a whole, because its entry names no `via` symbol the
repository has. It shows as `(repository-level)` and a trace does not enter it.
_Avoid_: global, whole-repo

**Link**:
A connection Meada makes between a consumer and a provider through a contract. It carries a
level and a reason.
_Avoid_: edge, bridge, connection

**Level**:
How well a link is established: exact, declared, inferred, decided or ambiguous. It is a name,
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

**Stale decision**:
A decision that no longer applies, for one cause: its call site is gone, its item is no longer
open (the core now resolves the call on its own), its target is gone, or its target is no longer
a candidate of the ambiguous link. Indexing ignores it, so the link stays as the core sees it;
`meada status` and `meada export` report it with the cause. A decision whose target repository
is a missing peer is neither applied nor stale: it waits.
_Avoid_: outdated decision, invalid decision

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
