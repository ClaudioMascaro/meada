# Output grammar

This document specifies every line an agent can receive from the three MCP tools, `search`,
`context` and `trace`: the line kinds of each tool, the link notation, the levels, the contract
identifiers, the `search` kinds, the chain footer, the note of a budget cut and the token
estimate. It is in English, like the MCP surface
([ADR 0008](../adr/0008-mcp-surface-and-output-languages.md)). The
[stability document](../site/en/stability.md) says which of it is frozen in 1.x.

The tables of this document are also the grammar check's input: `tests/it/grammar.rs` reads
them, matches every line of every rendered-answer golden in `tests/golden/` against the line
kinds of its tool, and fails on a line no kind matches, naming the golden and the line. A
line kind documented here that no golden shows fails it too, unless the check names the test
that pins it instead. A change to a line kind changes this document in the same change.

## Reading an answer

- An answer is UTF-8 text made of lines, each ending in a newline. A line of `search` or `trace`
  is never empty; a line of a symbol's span text in `context` may be.
- **Skip what you do not know.** A minor version may add line kinds, fields at the end of a line
  kind, `search` kinds, and forms of a field (a contract kind is a new form of `<contract>`). A
  reader skips a line that matches no line kind it knows, and ignores a field or a form it does
  not know, instead of failing.
- A call the tool refuses (an unknown `kind`, `trace` with both `from` and `changes` or with
  neither, an index that cannot be read) returns an MCP error result, not an answer: its text is
  a message for the reader and follows no line kind of this document.
- Every answer is deterministic: the same files give the same bytes.

### Token estimate and budget

- The token estimate is one token per four bytes of the UTF-8 output, newlines included,
  rounded up: an answer of 401 bytes is 101 tokens. It is deterministic and needs no tokenizer.
- Every call takes a `budget` in tokens, 2,000 by default. An answer within its budget is
  complete. An answer that would exceed it keeps the lines that fit and ends with the **cut
  note** (a `trace` answer keeps its footer after it), within the budget. A budget too small for
  the cut note itself (or for the note and the footer) gives them all the same, so such an answer
  is larger than its budget.

## Notation

A line kind is a pattern written as code. Text stands for itself, spaces included (a pattern or a
form that starts with a space starts with that space); `<field>` stands for a field, and
`<field?>` for the field or nothing. A pattern matches a whole line.

Each field is defined in a table with the columns Field, Form and Meaning, near the section that
introduces it. A field with several rows takes the form of any one of them; a form is itself a
pattern. The forms at the bottom are the terminals:

| Terminal | Matches |
| --- | --- |
| `<word>` | One or more characters, none of them a space. |
| `<number>` | One or more ASCII digits. |
| `<text>` | One or more characters, spaces included. |
| `<indent>` | Two spaces, repeated zero or more times. |

A name, a label or a reason is `<text>`: it may hold spaces (a route handler is named
`GET /api/users`, a Kotlin parameter list may read `(vararg String)`). The fixed words that
follow it on its line are what ends it, so a reader reads such a line from both ends. Meada
prints paths and identities as they are, unquoted; the grammar takes them as `<word>`, so a
file whose path holds a space reads as more fields than it has.

### Fields every tool uses

| Field | Form | Meaning |
| --- | --- | --- |
| `<repository>` | `<word>` | A repository identity, such as `example.com/acme/web`. |
| `<path>` | `<word>` | A file path relative to its repository's root, with `/` separators. |
| `<line>` | `<number>` | A 1-based line of a file. |
| `<place>` | `<path>:<line>` | Where something is: the line of a file. |
| `<place>` | `<path>` | A file Meada cannot read as it answers (removed since indexing, as in a mirror server mode has not indexed again), so the line is unknown. |
| `<name>` | `<text>` | A symbol's name as indexed: qualified with dots (`Cart.total`), with its parameter types in languages with overloads (`Orders.find(String)`), a route handler with its route (`GET /api/users`). |
| `<caller>` | `<name>` | The symbol a call sits in. |
| `<caller>` | `(top level)` | A call outside any symbol, at the top level of its file. |
| `<callee>` | `<text>` | What a call names, as written (`fetch`, `requests.get`). |
| `<identity>` | `<repository>::<path>::<name>` | A symbol's identity. |
| `<identity>` | `<repository>::<path>` | A file's identity: a route file, or the top level of a file. |
| `<n>` | `<number>` | A count. |
| `<query>` | `<text>` | What the call was given, echoed. |

## Levels

Every link and every call that is not certain carries a level. The levels, strongest first:

| Field | Form | Meaning |
| --- | --- | --- |
| `<level>` | `exact` | Both sides match as written. |
| `<level>` | `declared` | The link comes from `meada.toml`, such as a shared resource. |
| `<level>` | `inferred` | The sides match after Meada resolved a base URL or a prefix; a call matched by its name alone. |
| `<level>` | `decided` | The link comes from `meada.decisions.toml`, a person's or a model's choice. |
| `<level>` | `ambiguous` | Several candidates fit: they are listed, never silently picked. |

A chain is as strong as its weakest link: the footer of `trace` names the greatest level of the
lines it shows, in this order.

## Links and marks

A **link** joins a consumer to a provider through a contract. The **link notation** is the
level and the reason in brackets after an arrow, written before the line the link reaches (in
`trace`) or before the provider it reaches (in `context`):

| Field | Form | Meaning |
| --- | --- | --- |
| `<arrow>` | `→[<level>: <reason>]` | A link at `<level>`, for `<reason>`. |
| `<reason>` | `<text>` | Why the link holds, for a person to read. |
| `<mark>` | ` [<level>: <reason>]` | After a call or a side that is not exact: its level and why. |

The reason of a link takes forms such as `host api.example.com is served by <repository>`,
`base URL from API_URL, a base_url_env of peer <repository>`, `host unresolved; the only
repository serving it is <repository>`, `2 providers fit; host unresolved; served by
<repository>`, `decided by human`, `decided by <model> (p=0.9)`, the contract itself (a declared
resource, a channel), `<channel> wildcard <pattern>`, `<queue> binding <exchange> <pattern>`,
and for a package `<contract> <range> (local <version>)`, followed by `, range unchecked` or
`, range mismatch` when the range does not plainly accept the version, and `; import <how>` for
an import hop. A link to a provider that serves every method ends with `; any method`.

The reason of a call's mark is how many definitions have its name: `one method named render`,
`2 functions or classes named render`, `one of several methods named run` (a match narrowed to
one).

## Contract identifiers

A contract identifier starts with its contract kind and `::`:

| Field | Form | Meaning |
| --- | --- | --- |
| `<contract>` | `http::<method>::<route>` | An HTTP route: `http::GET::/api/users/{}`. |
| `<contract>` | `http::ANY::<route>` | A route a provider serves on every method: it fits a consumer of any method. |
| `<contract>` | `channel::<broker>::<channel>` | A queue, topic or subject of a broker: `channel::kafka::orders.created`. |
| `<contract>` | `package::<ecosystem>::<package>` | A package: `package::npm::@example/kit`. |
| `<contract>` | `resource::<resource-kind>::<resource-id>` | A resource declared in `meada.toml`: `resource::postgres::orders`. |
| `<method>` | `<word>` | An HTTP method in upper case, such as `GET` or `POST`. |
| `<route>` | `<word>` | A route path; each parameter is `{}`. |
| `<broker>` | `<word>` | The broker kind: `kafka`, `amqp`, `nats`, `sqs`, `servicebus` and the others the language packs read. |
| `<channel>` | `<word>` | The channel's name; a pattern keeps its wildcards (`orders.>`). |
| `<ecosystem>` | `<word>` | `npm`, `pypi`, `go`, `cargo`, `maven`, `nuget` or `swiftpm`. |
| `<package>` | `<word>` | The package's name in its ecosystem: `@example/kit`, `example:models`, a Go module path. |
| `<resource-kind>` | `<word>` | The resource's `kind` in `meada.toml`. |
| `<resource-id>` | `<text>` | The resource's `id` in `meada.toml`. |
| `<shown-contract>` | `<contract>` | A contract as answers show it. |
| `<shown-contract>` | `channel::<broker> (unresolved)` | A channel side whose name Meada cannot read. |
| `<shown-contract>` | `(unresolved) <callee>` | In `search`: a consumer whose contract Meada cannot read (an open item), with the call it sits at. |

## `search`

`search` takes a `query`, matched case-insensitively (exact matches first, then prefixes, then
substrings), and an optional `kind`:

| `kind` | Lists |
| --- | --- |
| `symbol` | Symbols whose name matches. |
| `contract` | Providers and consumers whose contract identifier matches. |
| `route` | Providers and consumers of `http::` contracts. |
| `resource` | Writers and readers of `resource::` contracts. |
| `channel` | Producers and consumers of `channel::` contracts. |
| `document` | OpenAPI and AsyncAPI documents, by path or title. |
| `package` | Packages, by name. |
| `drift` | Fields a consumer reads that its provider lacks, by route or field path. |
| `community` | Communities, by name. |
| `process` | Processes, by their entry point's label. |
| `duplicate` | Groups of symbols with the same span text, by name or path. |
| `ambiguous` | Open items: calls with several candidate providers. |
| `unresolved` | Open items: calls whose target Meada cannot read. |

With no `kind`, `search` lists symbols and contracts. The answer is one line per match, or a
`no matches` line:

| Kind | Line | Meaning |
| --- | --- | --- |
| symbol | `<repository> <place> <name>` | A symbol, at its definition. |
| contract | `<repository> <site> <shown-contract>` | A provider or consumer of a contract, at its site. |
| document | `<repository> <path> (<summary>)<title?>` | A provider document. |
| package | `<repository> <directory> <package><at-version?><in-ecosystem?> (consumers: <n>)` | A package and the number of dependency entries that link to its manifest. |
| drift | `<repository> <place> <caller> <contract> missing <field-path> (<level>)` | One field path a consumer reads that the provider's shape lacks, with the level of that report. |
| community | `<repository> <community> (members: <n>)` | A community and its size. |
| process | `<repository> <label> process::<entry-identity> (<entry-kind>)` | A process: `process::<entry-identity>` is what `trace` starts from. |
| copies | `<n> copies` | A group of duplicates; one symbol line per copy follows. |
| open item | `` <repository> <place> (<item-kind>) candidates: <n>; run `meada decide --item <call-site>` `` | An open item and the command that closes it. |
| no matches | `` no matches for `<query?>` `` | Nothing matched. |

| Field | Form | Meaning |
| --- | --- | --- |
| `<site>` | `<place>` | Where the side is in code. |
| `<site>` | `<path>` | A route file serving with no function of its own, or a manifest. |
| `<site>` | `(repository-level)` | A side declared with no `via`: the whole repository. |
| `<site>` | `<path> (documented only)` | A side only a provider document lists. |
| `<site>` | `<path> <manifest-field>` | A dependency entry of a manifest, such as `package.json dependencies`. |
| `<manifest-field>` | `<word>` | The manifest's field holding the entry. |
| `<summary>` | `openapi <version>, routes: <n>` | An OpenAPI document and how many routes it lists. |
| `<summary>` | `asyncapi <version>, channels: <n>` | An AsyncAPI document and how many channels it lists. |
| `<version>` | `<word>` | A version, as written. |
| `<title>` | ` <text>` | The document's `info.title`. |
| `<directory>` | `<word>` | The manifest's directory, `.` at the root. |
| `<at-version>` | `@<version>` | The manifest's version. |
| `<in-ecosystem>` | ` (<ecosystem>)` | A package of another ecosystem than npm's. |
| `<field-path>` | `<word>` | A path into a shape: `items[].name`; `(root)` is the shape itself. |
| `<community>` | `<word>` | A community's name: its most common directory, `src#2` when another community of the repository has that name too. |
| `<label>` | `<text>` | An entry point's label: its route, channel or symbol name. |
| `<entry-identity>` | `<identity>` | An entry point's identity: its symbol's (an export, a program). |
| `<entry-identity>` | `<identity>::<contract>` | A route or channel handler's identity and the contract it handles. |
| `<entry-kind>` | `route` | A route handler. |
| `<entry-kind>` | `export` | An exported function or method nothing in its repository calls. |
| `<entry-kind>` | `channel` | A channel handler. |
| `<entry-kind>` | `program` | A program start a language pack declares or a manifest names. |
| `<item-kind>` | `ambiguous` | An ambiguous link: several providers fit. |
| `<item-kind>` | `unresolved` | An unresolved consumer, such as a dynamic URL. |

## `context`

`context` takes a `symbol`: a name, `<path>::<name>`, an identity, or a contract identifier as
`search` prints it. Its answer takes one of four forms.

**A symbol.** Its head line, its community line, its processes line, its span text, then its
sections:

1. the head: the symbol and the lines its span covers;
2. `community:`;
3. `processes:`;
4. the span text, verbatim: the lines from the range's first to its last, so a reader counts
   them from the head. A span cut by the budget keeps its first lines and ends with the cut note
   (`cut after <k> of <m> lines`, `<m>` the range's length); a span left out entirely is read
   from the file at that range. When the span text is unavailable the head shows `<place>` with
   no range, no span follows, and the line before `callers:` says why;
5. `callers:`, `callees:`, then, only when not empty, `annotations:`, `consumers:` and
   `contracts:`, each followed by its lines. `callers` and `callees` print `none` when empty.

The budget cuts the span text first, then the sections from the end; the head, the community and
the processes lines go last, only under a budget too small for them.

**Several symbols.** When the name fits several symbols, a `fits` line and one symbol line per
symbol, its name as `<path>::<name>` (or its identity when another repository has the same).

**A contract.** Its identifier, then one line per side across the workspace, providers first,
each followed by the shape the side carries and, for a route's consumers, its drift.

**Nothing.** A `no symbol` or `no contract` line.

In a symbol's answer, the section a line is under says its kind, since a name may hold any
words:

| Section | Line kinds |
| --- | --- |
| (before `callers:`) | unavailable, behind |
| `callers:` | caller |
| `callees:` | callee, overload set, unresolved call |
| `annotations:` | callee, overload set, unresolved call |
| `consumers:` | package consumer |
| `contracts:` | provides, shape fields, shape open, produces, publishes, consumes, consumes decided, consumes unresolved |

| Kind | Line | Meaning |
| --- | --- | --- |
| head | `<repository> <path>:<line>-<last> <name>` | The symbol and the lines `<line>` to `<last>` its span text covers. |
| symbol | `<repository> <place> <name>` | A symbol at its definition: the head when the span text is unavailable, or one of several symbols a name fits. |
| community | `community: <community>` | The symbol's community, `none` when it belongs to none. |
| no processes | `processes: none` | The symbol belongs to no process. |
| processes | `processes: <n> (<counts>; <n> <repositories>)<entries?>` | The processes the symbol belongs to, by entry kind and repository, then as many entries as fit. |
| unavailable | `(span text unavailable: <path> changed since indexing)` | The file changed after indexing. |
| behind | `index behind the mirror: run meada index --workspace` | Server mode: the mirror moved ahead of its index. |
| section | `<section>:` | The lines that follow belong to the section. |
| empty section | `<empty-section>: none` | The section has no line. |
| caller | `<repository> <place> <caller><mark?>` | A call that reaches the symbol. |
| callee | `<repository> <place> <name><mark?><on?>` | A symbol the symbol calls, at its definition; one line per candidate of an ambiguous call. |
| overload set | `<repository> <place> <name> (one of <n> overloads)<mark?><on?>` | The overloads of one method a call may reach, at the first one's definition. |
| unresolved call | `<repository> <place> (unresolved) <callee><on?>` | A call whose target Meada cannot read, at the call. |
| package consumer | `<repository> <place> <caller><tags?>` | A call in any repository that reaches the symbol through a package import. |
| provides | `<repository> <place> provides <contract>` | The symbol provides the contract. |
| shape fields | `<indent><repository> <shape-source> <carried> fields:<fields?>` | The field paths the shape carries. |
| shape open | `<indent><repository> <shape-source> <carried> open: <paths>` | The parts of the shape that are open: any field may be under them. |
| produces | `<repository> <place> produces <shown-contract><mark?>` | A call of the symbol produces to the channel; after a publish, a queue its bindings route it to. |
| publishes | `<repository> <place> publishes <exchange> <key>` | A call of the symbol publishes to an exchange with a routing key. |
| consumes | `<repository> <place> consumes <shown-contract>` | The symbol, or a call of it, consumes the contract. |
| consumes decided | `<repository> <place> consumes <contract> <arrow> <identity>` | A call of the symbol consumes the contract, linked by a decision to the provider `<identity>`. |
| consumes unresolved | `<repository> <place> consumes (unresolved) <callee>` | A call of the symbol consumes a contract Meada cannot read. |
| fits | ``` `<query>` fits <n> symbols; pass one of: ``` | The name fits several symbols, listed after it. |
| no symbol | `` no symbol matches `<query>` `` | No symbol has the name. |
| contract head | `<shown-contract>` | The contract the sides below belong to. |
| side | `  <repository> <role> <side><mark?>` | A side of the contract; a side linked through another name carries its mark. |
| drift | `    drift: missing <missing>` | Under a consumer: the field paths it reads that the provider lacks, each with its level. |
| not checked | `    not checked: <unchecked>` | Under a consumer: the field paths it reads under an open part of the shape. |
| no contract | `` no <contract-kind> matches `<query>` `` | No side of the contract is in the workspace. |

| Field | Form | Meaning |
| --- | --- | --- |
| `<last>` | `<number>` | The last line of the span. |
| `<counts>` | `<text>` | `<n> <entry-kind>` for each entry kind with processes, joined by `, `. |
| `<repositories>` | `repository` | One repository holds the entry points. |
| `<repositories>` | `repositories` | Several repositories do. |
| `<entries>` | `: <text>` | The processes, `<label> (<repository>)` each (its `process::<entry-identity>` when another process of that repository has the same label), joined by `, `, ending in `+<n> more` when not all fit. |
| `<section>` | `callers` | The calls that reach the symbol. |
| `<section>` | `callees` | What the symbol's calls reach. |
| `<section>` | `annotations` | The symbol's calls that decorate a definition (attributes, decorators, annotations). |
| `<section>` | `consumers` | The calls that reach the symbol through a package import. |
| `<section>` | `contracts` | The contracts the symbol provides and consumes. |
| `<empty-section>` | `callers` | No call reaches the symbol. |
| `<empty-section>` | `callees` | The symbol calls nothing. |
| `<on>` | ` on <name>` | The definition an annotation decorates. |
| `<tags>` | ` [<level>]` | The link is not exact. |
| `<tags>` | ` [<range-flag>]` | The dependency's range does not plainly accept the provider's version. |
| `<tags>` | ` [<level>, <range-flag>]` | Both. |
| `<range-flag>` | `range unchecked` | Meada cannot tell whether the range accepts the version. |
| `<range-flag>` | `range mismatch` | The range does not accept the version. |
| `<shape-source>` | `<path> (document)` | The shape comes from a provider document. |
| `<shape-source>` | `<path> (zod)` | From a zod schema. |
| `<shape-source>` | `<path> (literals)` | From the object literals the code returns. |
| `<shape-source>` | `<path> (reads)` | From the fields the code reads. |
| `<carried>` | `response` | What a route returns. |
| `<carried>` | `payload` | What a channel carries. |
| `<fields>` | ` <paths>` | The field paths, none when the shape has no field. |
| `<paths>` | `<text>` | Field paths joined by `, `. |
| `<exchange>` | `<word>` | An exchange. |
| `<key>` | `<word>` | A routing key. |
| `<role>` | `writer` | Writes the resource. |
| `<role>` | `reader` | Reads the resource. |
| `<role>` | `producer` | Produces to the channel: the symbol around the producing call. |
| `<role>` | `consumer` | Consumes the channel (its handler), or calls the route. |
| `<role>` | `provider` | Serves the route: its handler. |
| `<role>` | `package` | The manifest that declares the package. |
| `<role>` | `dependent` | A dependency entry on the package, or a call that reaches it through an import. |
| `<role>` | `binding <exchange> <binding-pattern>` | A binding that routes the exchange's messages to the queue. |
| `<binding-pattern>` | `<word>` | The binding's routing pattern. |
| `<side>` | `<place> <caller>` | A side in code: the line and the symbol around it. |
| `<side>` | `<path>` | A manifest. |
| `<side>` | `<path> (top level)` | A route file serving with no function of its own. |
| `<side>` | `(repository-level)` | A side declared with no `via`. |
| `<side>` | `<path> (documented only)` | A side only a provider document lists. |
| `<side>` | `<path> <manifest-field>` | A dependency entry of a manifest. |
| `<missing>` | `<text>` | `<field-path> (<level>)` each, joined by `, `. |
| `<unchecked>` | `<text>` | `<field-path> under <field-path>` each, the open part second, joined by `, `. |
| `<contract-kind>` | `http` | An HTTP route. |
| `<contract-kind>` | `channel` | A channel. |
| `<contract-kind>` | `resource` | A resource. |
| `<contract-kind>` | `package` | A package. |

## `trace`

`trace` takes a start (`from`, or `changes: uncommitted`) and a `direction`, `down` (what the
start reaches) or `up` (what reaches it). Its answer is a **chain** or a message.

A chain is one line per node the walk reaches, each indented two spaces per level of depth
below its start, which is at no indentation; a route several providers serve, or
`changes: uncommitted`, may give several starts. A line reached through a link begins with the
link notation. A node already shown above is written again, with `(see above)`, and not
followed; a node that is one of its own ancestors ends with `(cycle)`. When the budget cuts the
chain, the cut note follows the lines that fit. The **footer** closes the chain:

`chain: <chain-level> · <n> hops · <n> repos · <n> tokens`

`<chain-level>` is the greatest level among the lines shown (`none` when none has one); the
numbers are, in order, the lines shown that a link reached (the hops), the repositories the
lines shown are in, and the token estimate of the whole answer, footer included. Last come
the peers a `meada.toml` names that the workspace cannot find, one line each.

A trace from `process::<entry-identity>` (`down` only) begins with a `process` line and walks
the process: an ambiguous call is one line listing its candidates, and a call that is not exact
carries its level with no reason.

| Kind | Line | Meaning |
| --- | --- | --- |
| process | `process <label>` | The process whose steps follow. |
| node | `<indent><node><call-mark?><repeat?><drift?>` | A node the walk reaches through a call, or a start. |
| link | `<indent><arrow> <node><repeat?><drift?>` | A node the walk reaches through a link. |
| overloads | `<indent><repository> <place> <name> (<n> overloads)<call-mark?>` | The overloads of one method a call may reach; the walk does not follow them. |
| unresolved | `<indent><repository> <place> (unresolved) <callee>` | A call whose target Meada cannot read. |
| no provider | `<indent><repository> <place> consumes <contract>, no provider` | A call that consumes a contract no repository of the workspace provides. |
| channel side | `<indent><repository> <place> <verb> <shown-contract>` | A call or handler that produces to or consumes a channel and links to no other side. |
| candidates | `<indent><repository> <place> <callee><step-level?>: <candidates>` | A process step: an ambiguous call and its candidates. |
| footer | `chain: <chain-level> · <n> hops · <n> repos · <n> tokens` | The chain footer. |
| missing peer | `missing peer: <repository>, named by <repository>` | A peer the second repository's `meada.toml` names that the workspace cannot find. |
| no overlap | `no symbol overlaps the uncommitted changes` | `changes: uncommitted` found no symbol to start from. |
| process up | `a process is what its entry point sets in motion: trace it down` | A process traces `down` only. |
| no process | `` no process `<query>`: pass `process::<entry point identity>` `` | No entry point has that identity. |
| not a start | ``` `<query>` is a <sided-kind>, and a trace starts from a symbol, a route or a call site: run `context` on it for its <sides>, then trace from one ``` | A contract listed by its sides is no start. |
| fits symbols | ``` `<query>` fits <n> symbols; pass one of: ``` | The name fits several symbols, listed after it. |
| fit symbol | `<node> <identity>` | One of the symbols the name fits, and its identity. |
| fits call sites | ``` `<query>` fits <n> call sites; pass one of: ``` | The name fits several call sites, listed after it. |
| fit call site | `<repository> <place> <call-site>` | One of the call sites the name fits. |
| no start | `` no symbol, route or call site matches `<query>` `` | Nothing to start from. |

| Field | Form | Meaning |
| --- | --- | --- |
| `<node>` | `<repository> <place> <name>` | A symbol at its definition, or a call site and the name it calls. |
| `<node>` | `<repository> <path> (top level)` | A file: the top level of a file making calls, or a route file serving with no function of its own. |
| `<node>` | `<repository> (repository-level)` | A side declared with no `via`; the branch ends there. |
| `<node>` | `<repository> <path> <name> (documented only)` | A side only a provider document lists, such as `GET /x` or `kafka send orders.created`; the branch ends there. |
| `<call-mark>` | `<mark>` | The call that reaches the node is not exact. |
| `<call-mark>` | `<step-level>` | In a process: the call's level, with no reason. |
| `<step-level>` | ` [<level>]` | The level of a call that is not exact. |
| `<repeat>` | ` (cycle)` | The node is one of its own ancestors. |
| `<repeat>` | ` (see above)` | The node is shown above, with what follows it. |
| `<repeat>` | ` (depth limit)` | A process step at the walk's depth limit: what follows it is not shown. |
| `<drift>` | ` drift: missing <paths>` | The link's consumer reads fields its provider lacks. |
| `<verb>` | `produces` | Produces to the channel. |
| `<verb>` | `consumes` | Consumes the channel. |
| `<candidates>` | `<text>` | The candidates' identities, an overload set as `<name> (<n> overloads)`, joined by `, `. |
| `<chain-level>` | `<level>` | The weakest level of the lines shown. |
| `<chain-level>` | `none` | No line shown has a level. |
| `<sided-kind>` | `resource` | A resource. |
| `<sided-kind>` | `channel` | A channel. |
| `<sided-kind>` | `package` | A package. |
| `<sides>` | `writers and readers` | A resource's sides. |
| `<sides>` | `producers and consumers` | A channel's sides. |
| `<sides>` | `packages and dependents` | A package's sides. |
| `<call-site>` | `<identity>::<callee>#<n>` | A call site's identity: its caller's (a symbol, or a file for a call at its top level), what it calls, and its ordinal among the caller's calls of that name. |

## Every tool

| Kind | Line | Meaning |
| --- | --- | --- |
| cut | `… cut after <n> of <n> lines (budget <n> tokens); raise budget for more` | The budget cut the answer (or a symbol's span text): it kept the first number of lines of the second, under the budget named. |
