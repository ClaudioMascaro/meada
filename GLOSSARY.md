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
queries that read what one file defines, imports, exports and calls. It holds syntax and value
evaluation only: no table of libraries and no form of one (those belong to the contract kinds).
It never touches the index. A pack is done when it passes the conformance suite over its fixture.
_Avoid_: plugin, parser, grammar

**Language adapter**:
The seam between a language pack and Meada's shared scope and import resolution, which
connects each call to the definitions it reaches and gives the call a level.
_Avoid_: driver, frontend, bridge

**Family**:
A class and its ancestors in method resolution order (C3), which is where a member of the class
is looked up. The index keeps the family of each public class whose overloads merge across it
(Java) in the `families` table, so an import from another repository picks the method among the
overloads of the whole family; an ancestor from outside the repository is opaque.
_Avoid_: hierarchy, inheritance chain

**Injector**:
What a contract kind reads off an annotation written on a field or a parameter while it folds a
value (`resolve::Inject`): Spring's `@Value("${key}")` holds the key's value from the module's
configuration. What it gives lives only while the contracts are extracted.
_Avoid_: overlay, binder

**Community**:
A group of symbols in one repository that call each other more than they call the rest, found
deterministically from the calls at `exact` or `inferred` and named after their most common
directory (`src`, `src#2` when another community of the repository has that name too). Every
symbol in at least one such call, as caller or callee, is a member, so one whose only such call
is to itself, or comes from top-level code, is a community of its own; a symbol in no such call
belongs to none.
_Avoid_: cluster, module, component

**Server mode**:
Meada run once for an organisation: `meada mcp --listen` serves the tools over HTTP from mirrors
of the repositories that a workspace manifest lists, each index outside its checkout, refreshed
by `meada index` rather than on every query (ADR 0011). With the snapshot module, nodes do not
run `meada index`: they serve generations a job published (ADR 0012).
_Avoid_: hosted mode, cloud, daemon

**Cloud module**:
An opt-in part of server mode for a deployment on several nodes: a cargo feature, off by
default, plus one section of the bootstrap manifest (the snapshot store, the generation
pointer, the refresh lock, OIDC, the console). A build without the feature carries none of its crates, and with no section a build behaves as the default one (ADR 0012).
_Avoid_: plugin, extension, cloud mode

**Generation**:
The workspace's mirrors and indexes at one commit each, published by one job and served whole
by every node: one bundle per repository, never merged, named by a 16-hex id derived from the
commits, the schema version, the binary version and the entries' blocks (ADR 0012). A client
pins one with `/<identity>@<generation>`.
_Avoid_: snapshot, version, release

**Snapshot store**:
The S3-compatible bucket and prefix the operator configures, where `meada snapshot publish`
uploads generations and from which `meada snapshot pull` downloads them onto a node.
_Avoid_: cache, registry, artifact store

**Refresh lock**:
The lock in Valkey that `meada snapshot lock` holds while the operator's refresh (fetch, `meada
index`, `meada snapshot publish`) runs as its child, so one refresh of a workspace runs at a
time (ADR 0012).
_Avoid_: mutex, lease, job lock

**Fence**:
The number each acquisition of the refresh lock gets, one above the last. The pointer in Valkey
moves only to a higher fence and a node's never to a lower one, so a stalled job never replaces
a newer generation; without the lock every fence is `0`.
_Avoid_: epoch, version, fencing number

**GitHub App**:
A `[[github_app]]` block of the bootstrap manifest: the git host it serves, its REST `api`, its
App id and its private key. With the module, Meada answers the indexing job's git credential
requests with tokens of the App's installations (ADR 0013).
_Avoid_: connection, integration, bot

**Installation**:
A GitHub App installed on an owner (a user or an organisation), GitHub's own term. Meada lists
each App's installations from the GitHub API, keeps them in Valkey, and mints an installation
token for the one it picks for a repository of that owner.
_Avoid_: connection, account, tenant

**Node**:
One process serving the tools in server mode, with the companion that pulls its generations
when it serves them. Nodes only read; the console is not one.
_Avoid_: pod, replica, instance, server

**Heartbeat**:
What a node's pull companion leaves in Valkey after each round: its name, the generation and
fence it serves, the generations it holds, whether it is ready, its version and the time. It
expires soon after the node stops writing it, so a dead node drops out of the fleet on its own.
The console sees the fleet only through heartbeats; it never calls a node.
_Avoid_: status, ping, health check

**Console**:
The management console of server mode: a process of its own, beside the nodes and behind its own
admission, that shows people in a browser the fleet, the snapshot store, admission, the
workspace and the graph, and carries out an operator's changes through the same code as the
CLI's commands. It holds generations as a node does, but it is no node and no MCP server
(ADR 0014).
_Avoid_: dashboard, admin panel, UI

**Operator**:
Whoever runs a server mode deployment and owns its boundary, its admission, its services and
its job. In the console, the global operator is the role that may make every write, held by the
admitted people the identity provider marks with the group or claim the console names; a team
operator may change only its teams' entries (ADR 0015).
_Avoid_: admin, superuser

**Viewer**:
A person the console admits who is neither the global operator nor a team operator: reads every
view, within what a node would answer their token, and writes nothing.
_Avoid_: reader, guest, read-only user

**Owner**:
The OIDC group, read from the token's `groups_claim`, that an entry of the workspace version
names as the team answering for that repository. An entry has at most one owner; one without an
owner is the global operator's. The owner is part of the workspace version, never of a
generation, and never widens what a token may read (ADR 0015).
_Avoid_: tenant, maintainer, team lead, CODEOWNER

**Team operator**:
A person the console admits, not the global operator, one of whose groups owns an entry of the
current workspace version that their grant reads. They may edit, remove and add entries their
teams own and nothing else: no owner change, no entry without an owner, no generation operation.
A group becomes a team when the global operator assigns it its first entry.
_Avoid_: team admin, team lead, maintainer, tenant admin

**Identity provider**:
The one token issuer of a server mode deployment, configured in `[oidc]` with its `type`
(generic, Entra ID, Okta or Keycloak) and the table of settings that type needs. It issues the
tokens nodes and the console admit, carries the claims grant (`identities_claim` and
`[oidc.groups]`) and, with the directory, pushes its users and groups to the console. Always
written in full: "provider" alone is the side of a contract (ADR 0016).
_Avoid_: provider, IdP, auth provider, login provider

**Grant provider**:
A source of a token's grant: the identity provider's claims grant, and the GitHub grant provider
(`[oidc.github]`), which grants what the token's GitHub login holds an accepted role on, in the
permissions set the job's role check wrote at the last refresh. A token's grant is the union of
every configured grant provider's identities. Always written in full, never "provider" (ADR
0016).
_Avoid_: provider, grant source, permission source, authorizer

**Data layer**:
Postgres as a cloud module (`[postgres]`): the durable mutable state of server mode, beside the
immutable snapshot store and the volatile Valkey, holding the audit, the permissions set and the
directory in one schema that only `meada postgres migrate` changes. The console, the job's role
check and the CLI twins that write connect to it; nodes only read it; `meada index` never
connects (ADR 0016).
_Avoid_: database, persistence layer, state store

**Directory**:
The users and groups the identity provider pushes into the data layer through the console's
SCIM 2.0 endpoint (`[directory]`). It feeds the owner picker, flags an owner that names no group,
deprovisions a user the identity provider deactivated and gives an Entra ID overage token its
groups. Meada never pulls it: inbound only (ADR 0016).
_Avoid_: user store, LDAP, address book, IdP sync

### Across repositories

**Peer**:
A repository that another repository's `meada.toml` names as one it shares contracts with.
_Avoid_: member, group, neighbour, dependency

**Workspace**:
The repositories one query can see: the repository where Meada runs, its peers through their
`meada.toml` files (transitively), and the sibling clones whose `meada.toml` names a repository
already in it. Each keeps its own index.
_Avoid_: group, monorepo, organization

**Workspace manifest**:
What lists, for server mode, the repositories of the workspace (remote identity and local path)
and the `[serve]`, `[[resource]]` and `[[peer]]` (`repo` and `base_url_env`, no `path`) of
repositories with no `meada.toml`. By default it is part of the bootstrap manifest, the
versioned `meada.workspace.toml` in an operator's ops repository; when the workspace is kept in
the snapshot store, it is the current workspace version. A repository's own `meada.toml` wins
over it whole. It is never read locally.
_Avoid_: group config, central repository, global configuration

**Bootstrap manifest**:
The file a server mode command is given with `--workspace`: the index directory, the sections of
the cloud modules and of the console, and either the repositories themselves or the choice to
take them from the current workspace version. It holds no secret, and the console never edits
it.
_Avoid_: config file, settings, deployment manifest

**Workspace version**:
One immutable list of the workspace's repositories and their blocks, kept in the snapshot store
with its author, time and message and named by a 16-hex id derived from the entries alone, so
the same list gets the same id anywhere. One version is current at a time; applying an older one
is the rollback, and each generation records the version that produced it.
_Avoid_: revision, snapshot, manifest version

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

**Declarative client**:
An interface whose methods are the requests, such as an OpenFeign or a Retrofit interface. A
method of one has no call of its own, so it is the consumer, sited at the method; a caller of the
method reaches it as any call, and an open item on it shows the method.
_Avoid_: client interface, proxy

**Client wrapper**:
A function whose call to an HTTP client (`fetch`, `requests`, `httpx` or `aiohttp` in
Python, RestTemplate, WebClient, `java.net.http` or OkHttp in Java) or a message client (a Kafka producer) takes its URL, method or channel name from the
function's own parameters. Each call site of the wrapper is a consumer or producer, read with its
arguments in place of the parameters, through any number of wrapper levels.
_Avoid_: API client, helper, fetcher

**Origin**:
What made a value the code reads: the call of a library's constructor or factory, or what the
source declares a name is, with the methods called on the way. A contract kind accepts a router,
client or producer only on an origin it knows. The evidence the walk accepts is a policy the
reader picks: the flow of values (constants, imports, arguments, returns), `const` bindings
alone, or the declarations the source writes.
_Avoid_: maker, provenance

**Contract**:
What one repository exposes and another depends on, such as an HTTP route, a channel, a package
or a shared resource.
_Avoid_: interface, integration

**Contract kind**:
A category of contract with its own matching rules, such as HTTP route, channel or package.
A kind owns the library forms it reads in each language (the web frameworks and clients of HTTP,
the broker clients of channels), one module per language.
_Avoid_: contract type, protocol

**Package ecosystem**:
The registry a package is published to, which decides the manifests that declare it and the
second part of its contract: npm (`package.json`, `package::npm::<name>`), PyPI
(`pyproject.toml` and `requirements*.txt`, `package::pypi::<name>`), Cargo (`Cargo.toml`,
`package::cargo::<name>`), Go (`go.mod`, `package::go::<name>`), Maven (`pom.xml` and
Gradle's build files, `package::maven::<group>:<artifact>`) NuGet (SDK-style `.csproj`,
`package::nuget::<id>`) or SwiftPM (`Package.swift`, `package::swiftpm::<name>`). An ecosystem is one module
behind the `Ecosystem` trait, registered in a list. A dependency links only to a package of
its own ecosystem, so an npm and a PyPI package of one name never link.
_Avoid_: registry, package manager, language

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

**Entry point**:
A door into a repository: each route an HTTP handler serves (kind `route`, the handler a
function or a file), each channel a consumer handler consumes (kind `channel`), each program
start a language pack declares or a manifest names, such as a console script of a
`pyproject.toml` (kind `program`, a door even when called) and each exported
function or method that nothing in its repository calls, directly or through a package import
of the same repository, an ambiguous candidate counting as a caller and a self-call not (kind
`export`). It has an identity (the handler's identity plus, for a route or a channel, the
contract), a kind and a label (`GET /users/{}`, or the qualified name). No score or threshold
picks entry points: every one has its process.
_Avoid_: root, entrypoint, main

**Process**:
The ordered call tree one entry point sets in motion: its calls inside a repository at every
level and its links to other repositories, ending at leaves, an ambiguous call or link (one
step listing its candidates), an unresolved consumer, a cycle or a missing peer. It is addressed as `process::<entry point identity>`, named by its entry
point's label, and walked on demand from the stored calls and links, never stored.
_Avoid_: flow, workflow, execution path

### Decisions

**Open item**:
An ambiguous link or an unresolved consumer (a dynamic URL) of a repository that no decision
resolves. Its id is the call-site identity (a declarative client's method for a consumer sited
there), and its candidates are the providers the link lists
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
