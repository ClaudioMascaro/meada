# MCP output grammar

Normative reference for the successful English text returned by `search`, `context` and
`trace` (ADR 0008, spec #32). It describes the current producer. MCP argument-validation,
I/O and other tool-error results are outside this successful-answer grammar.

Consumers of 1.x must **skip line kinds and fields they do not know**, including newly added
fields on a known kind. They must not treat this development check as a forward-compatible
runtime validator: it deliberately rejects producer lines that have not been documented yet.
Removing or changing an existing kind's meaning is a breaking change; adding one is additive.

## Text and opaque fields

Output is UTF-8, newline terminated, with LF between lines. Numbers are decimal. Repository
identities are normalized remote identities, or `local/<directory>` without a remote.
A location is a repository-relative path and, when readable, a one-based `:line`. Symbol
headers add `:first-last`, inclusive. A location can instead be `(repository-level)`, a
provider document followed by `(documented only)`, or a package manifest and dependency key.
Names, paths, labels, reasons and field-path lists are unescaped opaque text: they may contain
spaces and punctuation. Do not infer a unique whitespace parser for arbitrary source or
names. Use the structured export when a consumer needs unambiguous identities and fields.

Two ASCII spaces denote one trace depth or a contract side. Shapes and drift below a side
use four spaces. Source lines keep their own indentation, blank lines and text, even text
that looks like a structural section. Blank separators in test goldens join several complete
answers; they are not an additional kind of query result.

## Contracts, levels and annotations

| Contract kind | Canonical identifier | Meaning |
| --- | --- | --- |
| HTTP | `http::<METHOD>::/<path>` | Uppercase method; normalized parameters are `{}`. `http::ANY::/path` means a provider serving every method, never `http::*::/path`. |
| Channel | `channel::<broker>::<name>` | Evaluated broker/name; current brokers are kafka, amqp, sqs, nats, redis and bullmq. `channel::<broker> (unresolved)` has no evaluated name and is never linked. |
| Package | `package::<ecosystem>::<name>` | npm, pypi, cargo or go; scoped names retain `@scope/name`. |
| Resource | `resource::<kind>::<id>` | Declared resource kind and identity. |

`process::<entry point identity>` addresses a process, not a contract. `trace` also accepts
human route aliases such as `GET /users/{}` and `/users/{}`; these are not canonical contract
identifiers. An unresolved channel's identifier is `channel::<broker>`; the text appends
` (unresolved)` when showing it. Unresolved HTTP consumers have no contract and show
`(unresolved) <callee>`.

The level vocabulary, strongest first, is **exact > declared > inferred > decided > ambiguous**.
A link is `→[<level>: <reason>]`, followed by its placed target. A match to an all-methods HTTP
provider adds `; any method` to the reason. Ordinary non-exact call rows append
` [<level>: <reason>]`; process call rows append ` [<level>]`. Ambiguous process calls/links
append ` [ambiguous]: <candidate identity>, ...` and do not expand candidates. Package consumer
annotations also include ` [range unchecked]`, ` [range mismatch]`, or
` [<level>, range unchecked]`. Reasons are explanatory opaque text, not another protocol.
Stops are `(cycle)`, `(see above)` and `(depth limit)`. A link can append
` drift: missing <field path>, ...`. Shape sources are `(document)`, `(zod)` or `(literals)`;
field paths use dotted names, `[]` for array elements, and `(root)` for the payload root.

## Search

`kind` accepts `symbol`, `contract`, `route`, `resource`, `channel`, `document`, `package`,
`drift`, `community`, `process`, `ambiguous` and `unresolved`. With no kind, search lists
symbols and placed provider/consumer contracts, including unresolved HTTP consumers. It does
not include every specialized kind. Matching is case-insensitive, exact before prefix before
substring, with deterministic identity tie breaks. Each hit occupies one line, followed by a
cut marker only when needed. The empty result says `no matches for`, followed by the query
in backticks.

Package hits show `<directory> <name>[@<version>]`, an ecosystem suffix except for npm, and
`(consumers: <n>)`. Document hits show `(openapi <version>, routes: <n>)` or
`(asyncapi <version>, channels: <n>)` and an optional title. Community hits carry member
counts; process hits carry the `process::` identity and entry point kind. A drift hit carries
consumer location/name, contract and `missing <field path> (<level>)`. Open items carry their
candidate count and the exact `meada decide --item <call-site identity>` command.

## Context and counted source

A symbol header is followed by `community:` and `processes:`. A process summary lists total
count, nonzero kind counts in **route, export, channel, program** order, and the number of
repositories. Entries are `<label> (<repository>)`, or a process identity when labels collide
inside one repository. `, +<n> more` lists how many entries the budget omitted; the summary
can stand alone. `community: none` and `processes: none` are explicit empty forms.

For a header with range `first-last`, the source has exactly `last - first + 1` lines. It is
one of: the whole counted block; a prefix followed by a cut whose `shown` equals that prefix
length and whose `total` equals the header's span length; or no source because the budget
kept the structural tail instead. A header without a range has no opaque source block and
uses the unavailable-span note or the server-mode index-behind note. The validator tries only
these counted alternatives and requires the remaining structural tail to validate.

The tail has `callers:` and `callees:`, with `: none` when empty, then optional `consumers:`
and `contracts:`. Caller/callee rows can name `(top level)` or an `(unresolved)` callee.
Contract rows use `provides`, `produces` and `consumes`; RabbitMQ exchange sends use
`publishes <exchange> <key>` and can additionally produce routed queue contracts. A decided
consume appends its decision arrow and provider identity. Shape lines follow their contract.

A contract context starts with its canonical identifier, then sides in two-space indentation:
writer/reader for resources, producer/consumer for channels, provider/consumer for HTTP, and
package/dependent for packages. Bindings list exchange, pattern and placed enclosing symbol.
Four-space shape/drift lines follow their side. `drift: missing` paths each carry a level;
`not checked: <path> under <open path>` is unchecked, never drift. A context ambiguity lists
candidates instead of guessing, and a no-match result has no symbol/contract body.

## Trace, footer and budget

A trace walks calls and cross-repository links, two spaces per depth. A process trace starts
with `process <label>`. Roots/calls show placed symbols or unresolved callees; links show the
arrow, repository and placed side. Repository-level and documented-only targets are explicit.
Unresolved channel rows use `produces` or `consumes`; a known contract without a provider is
`consumes <contract>, no provider`. Ambiguity/no-match/advisory results list candidates or
instructions and have no chain footer.

Every actual chain ends with:

```text
chain: <level|none> · <hops> hops · <repos> repos · <tokens> tokens
```

The level is the weakest displayed level, or `none` when none is present. Hops count displayed
link crossings, not ordinary calls; repositories count distinct repositories in displayed
rows. Optional `missing peer: <identity>, named by <identity>` lines follow the footer.
The footer's token count includes itself and these missing-peer lines, settled to the integer
that counts its own digits. It describes the displayed chain after any cut.

All three tools default to **2,000 tokens**. The estimate is
`ceil(output.as_bytes().len() / 4)`: one token per four UTF-8 bytes, rounded up, including
newlines. An answer cut by a budget uses exactly this marker (U+2026 ellipsis):

```text
… cut after <shown> of <total> lines (budget <n> tokens); raise budget for more
```

The source block can have its own cut; a trace's final cut precedes its footer. Context gives
up source before its structural tail and keeps its header/community/process summary as long
as possible. The marker/footer themselves can exceed a very small requested budget; an empty
or tiny budget is not a promise to erase this diagnostic text.

## Line kinds and executable check

The table and fenced JSON are one normative reference. `tests/it/grammar.rs` checks their
kind/state inventories agree, validates every rendered-answer golden (including newly added
ones), and rejects undocumented structural lines with file/line/state diagnostics. Only the
`source` state admits opaque lines, and only for the counted span described above. Named
captures in `symbol-header` and `cut` carry the span and prefix counts. A cut in a trace body
transitions to `trace-cut`, where a footer is still required; elsewhere it finishes the answer.
Terminal states are search, callee-rows, consumer-rows, contracts, sides, details, choice,
after-trace and done. Between complete concatenated golden answers a blank resets the state.

| Kind | States | Meaning |
| --- | --- | --- |
| `search-symbol` | `search` | Repository, source location and qualified symbol name, or `(unresolved)` and callee. |
| `search-contract` | `search` | Repository, placed side and canonical contract identifier. |
| `search-document` | `search` | Document path, format/version, route or channel count, optional title. |
| `search-package` | `search` | Package directory, name and optional version, non-npm ecosystem, consumer count. |
| `search-community` | `search` | Community name and member count. |
| `search-process` | `search` | Entry point label, process identity and entry point kind. |
| `search-drift` | `search` | Consumer location/name, contract, missing field path and drift level. |
| `open-item` | `search` | Open item, candidate count and exact command to decide its call-site identity. |
| `search-none` | `search` | No search match. |
| `symbol-header` | `context` | Symbol header whose inclusive source range counts the following span. |
| `unavailable-header` | `context` | Symbol header without a range: span text unavailable, no opaque source block. |
| `community` | `community` | Community name or `none`. |
| `processes` | `processes` | Process count, kind counts, repository count and optional entries/remaining count. |
| `source` | `source` | One opaque source line, accepted only within a counted full or cut source span. |
| `unavailable` | `unavailable` | Why no span text can be shown. |
| `callers-none` | `callers` | Empty caller section. |
| `callers` | `callers` | Caller rows follow. |
| `callees-none` | `callees`, `caller-rows` | Empty callee section. |
| `callees` | `callees`, `caller-rows` | Callee rows follow. |
| `consumers` | `callee-rows` | Package consumer rows follow. |
| `contracts` | `callee-rows`, `consumer-rows` | Contract/publish rows and their shapes follow. |
| `candidate` | `choice` | Placed disambiguation candidate and qualified/full identity. |
| `relation` | `caller-rows`, `callee-rows`, `consumer-rows` | Caller, callee, package consumer or disambiguation candidate, with optional annotation. |
| `symbol-contract` | `contracts` | Provided/produced/consumed contract, unresolved side, optional decision or binding level. |
| `publish` | `contracts` | RabbitMQ exchange and routing key, before/alongside routed queue contracts. |
| `side-shape` | `sides`, `details` | Shape of a contract side, indented four spaces. |
| `shape` | `contracts` | Shape source, response/payload field list or open paths. Four spaces under a contract side. |
| `contract-header` | `context` | Canonical contract whose sides follow. |
| `contract-side` | `sides`, `details` | Placed side of a contract, with optional name and level/reason. |
| `binding` | `sides`, `details` | RabbitMQ exchange, routing pattern and binding location/enclosing symbol. |
| `drift` | `sides`, `details` | Missing response field paths, each carrying its drift level. |
| `unchecked` | `sides`, `details` | Consumer paths below an open provider path, not classified as drift. |
| `context-none` | `context` | No matching symbol or contract. |
| `choice` | `context`, `trace` | Disambiguation candidates follow; no trace footer. |
| `trace-process` | `trace` | Label of the process being walked. |
| `trace-ambiguous` | `trace-rows` | Unexpanded ambiguous process call or link and its candidate identities. |
| `trace-row` | `trace`, `trace-rows`, `after-trace` | Call/link row at two spaces per depth, with target, annotations, stops and optional drift. |
| `trace-advice` | `trace` | No-match result or instruction to choose an entry/side; no chain footer. |
| `chain` | `trace`, `trace-rows`, `trace-cut` | Weakest displayed level, displayed link hops, repositories and full-answer token estimate. |
| `missing-peer` | `after-trace` | Missing peer and repository that named it. |
| `cut` | `search`, `context`, `community`, `processes`, `unavailable`, `callers`, `caller-rows`, `callees`, `callee-rows`, `consumer-rows`, `contracts`, `sides`, `details`, `choice`, `trace`, `trace-rows` | Budget cut. In a source prefix its counts must agree with the header; a trace footer still follows. |

```json meada-output-grammar
[
  {
    "kind": "search-symbol",
    "states": [
      "search"
    ],
    "pattern": "^[^\\s]+/[^\\s]+ (?:[^\\n]+:[1-9][0-9]*|[^\\n]+\\.[A-Za-z0-9]+(?: \\(documented only\\)| [A-Za-z][A-Za-z0-9.]*)?|\\(repository-level\\)) (?:\\(unresolved\\) [^\\n\\[\\]:;]+|\\(top level\\)|[^\\n\\[\\]:(]+)(?: \\[(?:(?:exact|declared|inferred|decided|ambiguous)(?:: [^\\n\\]]+)?(?:, range unchecked)?|range unchecked|range mismatch)\\])?$"
  },
  {
    "kind": "search-contract",
    "states": [
      "search"
    ],
    "pattern": "^[^\\s]+/[^\\s]+ (?:[^\\n]+:[1-9][0-9]*|[^\\n]+\\.[A-Za-z0-9]+(?: \\(documented only\\)| [A-Za-z][A-Za-z0-9.]*)?|\\(repository-level\\)) (?:http::[A-Z]+::/[^\\n]*|channel::[a-z0-9-]+(?:::[^\\n]+| \\(unresolved\\))|package::[a-z0-9-]+::[^\\n]+|resource::[^:\\n]+::[^\\n]+)$"
  },
  {
    "kind": "search-document",
    "states": [
      "search"
    ],
    "pattern": "^[^\\s]+/[^\\s]+ [^\\n]+ \\((?:openapi [^,\\n]+, routes|asyncapi [^,\\n]+, channels): [0-9]+\\)(?: [^\\n]+)?$"
  },
  {
    "kind": "search-package",
    "states": [
      "search"
    ],
    "pattern": "^[^\\s]+/[^\\s]+ [^\\n]+(?: \\((?:pypi|cargo|go)\\))? \\(consumers: [0-9]+\\)$"
  },
  {
    "kind": "search-community",
    "states": [
      "search"
    ],
    "pattern": "^[^\\s]+/[^\\s]+ [^\\n]+ \\(members: [0-9]+\\)$"
  },
  {
    "kind": "search-process",
    "states": [
      "search"
    ],
    "pattern": "^[^\\s]+/[^\\s]+ [^\\n]+ process::[^\\n]+ \\((?:route|export|channel|program)\\)$"
  },
  {
    "kind": "search-drift",
    "states": [
      "search"
    ],
    "pattern": "^[^\\s]+/[^\\s]+ (?:[^\\n]+:[1-9][0-9]*|[^\\n]+\\.[A-Za-z0-9]+(?: \\(documented only\\)| [A-Za-z][A-Za-z0-9.]*)?|\\(repository-level\\)) [^\\n]+ (?:http::[A-Z]+::/[^\\n]*|channel::[a-z0-9-]+(?:::[^\\n]+| \\(unresolved\\))|package::[a-z0-9-]+::[^\\n]+|resource::[^:\\n]+::[^\\n]+) missing [^\\n]+ \\((?:exact|declared|inferred|decided|ambiguous)\\)$"
  },
  {
    "kind": "open-item",
    "states": [
      "search"
    ],
    "pattern": "^[^\\s]+/[^\\s]+ (?:[^\\n]+:[1-9][0-9]*|[^\\n]+\\.[A-Za-z0-9]+(?: \\(documented only\\)| [A-Za-z][A-Za-z0-9.]*)?|\\(repository-level\\)) \\((?:ambiguous|unresolved)\\) candidates: [0-9]+; run `meada decide --item [^\\n]+`$"
  },
  {
    "kind": "search-none",
    "states": [
      "search"
    ],
    "pattern": "^no matches for `[^\\n]*`$",
    "next": "done"
  },
  {
    "kind": "symbol-header",
    "states": [
      "context"
    ],
    "pattern": "^[^\\s]+/[^\\s]+ [^\\n]+:(?P<first>[1-9][0-9]*)-(?P<last>[1-9][0-9]*) [^\\n]+$",
    "next": "community"
  },
  {
    "kind": "unavailable-header",
    "states": [
      "context"
    ],
    "pattern": "^[^\\s]+/[^\\s]+ (?:[^\\n]+:[1-9][0-9]*|[^\\n]+\\.[A-Za-z0-9]+(?: \\(documented only\\)| [A-Za-z][A-Za-z0-9.]*)?|\\(repository-level\\)) [^\\n]+$",
    "next": "community"
  },
  {
    "kind": "community",
    "states": [
      "community"
    ],
    "pattern": "^community: [^\\n]+$",
    "next": "processes"
  },
  {
    "kind": "processes",
    "states": [
      "processes"
    ],
    "pattern": "^processes: (?:none|[1-9][0-9]* \\([1-9][0-9]* (?:route|export|channel|program)(?:, [1-9][0-9]* (?:route|export|channel|program))*; [1-9][0-9]* repositor(?:y|ies)\\)(?:: [^\\n]+)?)$",
    "next": "source"
  },
  {
    "kind": "source",
    "states": [
      "source"
    ],
    "pattern": "^[^\\n]*$"
  },
  {
    "kind": "unavailable",
    "states": [
      "unavailable"
    ],
    "pattern": "^(?:\\(span text unavailable: [^\\n]+ changed since indexing\\)|index behind the mirror: run meada index --workspace)$",
    "next": "callers"
  },
  {
    "kind": "callers-none",
    "states": [
      "callers"
    ],
    "pattern": "^callers: none$",
    "next": "callees"
  },
  {
    "kind": "callers",
    "states": [
      "callers"
    ],
    "pattern": "^callers:$",
    "next": "caller-rows"
  },
  {
    "kind": "callees-none",
    "states": [
      "callees",
      "caller-rows"
    ],
    "pattern": "^callees: none$",
    "next": "callee-rows"
  },
  {
    "kind": "callees",
    "states": [
      "callees",
      "caller-rows"
    ],
    "pattern": "^callees:$",
    "next": "callee-rows"
  },
  {
    "kind": "consumers",
    "states": [
      "callee-rows"
    ],
    "pattern": "^consumers:$",
    "next": "consumer-rows"
  },
  {
    "kind": "contracts",
    "states": [
      "callee-rows",
      "consumer-rows"
    ],
    "pattern": "^contracts:$",
    "next": "contracts"
  },
  {
    "kind": "candidate",
    "states": [
      "choice"
    ],
    "pattern": "^[^\\s]+/[^\\s]+ (?:[^\\n]+:[1-9][0-9]*|[^\\n]+\\.[A-Za-z0-9]+(?: \\(documented only\\)| [A-Za-z][A-Za-z0-9.]*)?|\\(repository-level\\)) [^\\n]+::[^\\n]+$"
  },
  {
    "kind": "relation",
    "states": [
      "caller-rows",
      "callee-rows",
      "consumer-rows"
    ],
    "pattern": "^[^\\s]+/[^\\s]+ (?:[^\\n]+:[1-9][0-9]*|[^\\n]+\\.[A-Za-z0-9]+(?: \\(documented only\\)| [A-Za-z][A-Za-z0-9.]*)?|\\(repository-level\\)) (?:\\(unresolved\\) [^\\n\\[\\]:;]+|\\(top level\\)|[^\\n\\[\\]:(]+)(?: \\[(?:(?:exact|declared|inferred|decided|ambiguous)(?:: [^\\n\\]]+)?(?:, range unchecked)?|range unchecked|range mismatch)\\])?$"
  },
  {
    "kind": "symbol-contract",
    "states": [
      "contracts"
    ],
    "pattern": "^[^\\s]+/[^\\s]+ (?:[^\\n]+:[1-9][0-9]*|[^\\n]+\\.[A-Za-z0-9]+(?: \\(documented only\\)| [A-Za-z][A-Za-z0-9.]*)?|\\(repository-level\\)) (?:provides|produces|consumes) (?:(?:http::[A-Z]+::/[^\\n]*|channel::[a-z0-9-]+(?:::[^\\n]+| \\(unresolved\\))|package::[a-z0-9-]+::[^\\n]+|resource::[^:\\n]+::[^\\n]+)|\\(unresolved\\) [^\\n]+)(?: →\\[decided: [^\\n]+\\] [^\\n]+| \\[(?:exact|declared|inferred|decided|ambiguous): [^\\n]+\\])?$"
  },
  {
    "kind": "publish",
    "states": [
      "contracts"
    ],
    "pattern": "^[^\\s]+/[^\\s]+ (?:[^\\n]+:[1-9][0-9]*|[^\\n]+\\.[A-Za-z0-9]+(?: \\(documented only\\)| [A-Za-z][A-Za-z0-9.]*)?|\\(repository-level\\)) publishes [^\\n]* [^\\n]*$"
  },
  {
    "kind": "side-shape",
    "states": [
      "sides",
      "details"
    ],
    "pattern": "^    [^\\s]+/[^\\s]+ [^\\n]+ \\((?:document|zod|literals)\\) (?:response|payload) (?:fields:(?: [^\\n]+)?|open: [^\\n]+)$",
    "next": "details"
  },
  {
    "kind": "shape",
    "states": [
      "contracts"
    ],
    "pattern": "^[^\\s]+/[^\\s]+ [^\\n]+ \\((?:document|zod|literals)\\) (?:response|payload) (?:fields:(?: [^\\n]+)?|open: [^\\n]+)$"
  },
  {
    "kind": "contract-header",
    "states": [
      "context"
    ],
    "pattern": "^(?:http::[A-Z]+::/[^\\n]*|channel::[a-z0-9-]+(?:::[^\\n]+| \\(unresolved\\))|package::[a-z0-9-]+::[^\\n]+|resource::[^:\\n]+::[^\\n]+)$",
    "next": "sides"
  },
  {
    "kind": "contract-side",
    "states": [
      "sides",
      "details"
    ],
    "pattern": "^  [^\\s]+/[^\\s]+ (?:writer|reader|producer|consumer|provider|package|dependent) (?:[^\\n]+:[1-9][0-9]*|[^\\n]+\\.[A-Za-z0-9]+(?: \\(documented only\\)| [A-Za-z][A-Za-z0-9.]*)?|\\(repository-level\\))(?: [^\\n]+)?$",
    "next": "sides"
  },
  {
    "kind": "binding",
    "states": [
      "sides",
      "details"
    ],
    "pattern": "^  [^\\s]+/[^\\s]+ binding [^\\n]* [^\\n]* (?:[^\\n]+:[1-9][0-9]*|[^\\n]+\\.[A-Za-z0-9]+(?: \\(documented only\\)| [A-Za-z][A-Za-z0-9.]*)?|\\(repository-level\\)) [^\\n]+$",
    "next": "sides"
  },
  {
    "kind": "drift",
    "states": [
      "sides",
      "details"
    ],
    "pattern": "^    drift: missing [^\\n]+ \\((?:exact|declared|inferred|decided|ambiguous)\\)(?:, [^\\n]+ \\((?:exact|declared|inferred|decided|ambiguous)\\))*$",
    "next": "details"
  },
  {
    "kind": "unchecked",
    "states": [
      "sides",
      "details"
    ],
    "pattern": "^    not checked: [^\\n]+ under [^\\n]+$",
    "next": "details"
  },
  {
    "kind": "context-none",
    "states": [
      "context"
    ],
    "pattern": "^no (?:symbol matches|(?:http|channel|package|resource) matches) `[^\\n]*`$",
    "next": "done"
  },
  {
    "kind": "choice",
    "states": [
      "context",
      "trace"
    ],
    "pattern": "^`[^\\n]*` fits [0-9]+ (?:symbols|call sites); pass one of:$",
    "next": "choice"
  },
  {
    "kind": "trace-process",
    "states": [
      "trace"
    ],
    "pattern": "^process [^\\n]+$",
    "next": "trace-rows"
  },
  {
    "kind": "trace-ambiguous",
    "states": [
      "trace-rows"
    ],
    "pattern": "^(?:  )*[^\\s]+/[^\\s]+ (?:[^\\n]+:[1-9][0-9]*|[^\\n]+\\.[A-Za-z0-9]+(?: \\(documented only\\)| [A-Za-z][A-Za-z0-9.]*)?|\\(repository-level\\)) [^\\n\\[:]+ \\[ambiguous\\]: [^\\n]+$",
    "next": "trace-rows"
  },
  {
    "kind": "trace-row",
    "states": [
      "trace",
      "trace-rows",
      "after-trace"
    ],
    "pattern": "^(?:  )*(?:→\\[(?:exact|declared|inferred|decided|ambiguous): [^\\n]+\\] )?[^\\s]+/[^\\s]+ (?:(?:[^\\n]+:[1-9][0-9]*|[^\\n]+\\.[A-Za-z0-9]+(?: \\(documented only\\)| [A-Za-z][A-Za-z0-9.]*)?|\\(repository-level\\))(?: (?:(?:\\(unresolved\\) [^\\n\\[\\]:;]+|\\(top level\\)|[^\\n\\[\\]:(]+)(?: \\[(?:(?:exact|declared|inferred|decided|ambiguous)(?:: [^\\n\\]]+)?(?:, range unchecked)?|range unchecked|range mismatch)\\])?|(?:produces|consumes) (?:http::[A-Z]+::/[^\\n]*|channel::[a-z0-9-]+(?:::[^\\n]+| \\(unresolved\\))|package::[a-z0-9-]+::[^\\n]+|resource::[^:\\n]+::[^\\n]+)(?:, no provider)?))?|[^\\n]+ \\(documented only\\))(?: \\((?:cycle|see above|depth limit)\\))?(?: drift: missing [^\\n]+)?$",
    "next": "trace-rows"
  },
  {
    "kind": "trace-advice",
    "states": [
      "trace"
    ],
    "pattern": "^(?:no symbol overlaps the uncommitted changes|no symbol, route or call site matches `[^\\n]*`|no process `[^\\n]*`: pass `process::<entry point identity>`|a process is what its entry point sets in motion: trace it down|`[^\\n]+` is a (?:http|resource|channel|package), and a trace starts from a symbol, a route or a call site: run `context` on it for its [a-z]+ and [a-z]+, then trace from one)$",
    "next": "done"
  },
  {
    "kind": "chain",
    "states": [
      "trace",
      "trace-rows",
      "trace-cut"
    ],
    "pattern": "^chain: (?:(?:exact|declared|inferred|decided|ambiguous)|none) · [0-9]+ hops · [0-9]+ repos · [0-9]+ tokens$",
    "next": "after-trace"
  },
  {
    "kind": "missing-peer",
    "states": [
      "after-trace"
    ],
    "pattern": "^missing peer: [^\\s]+/[^\\s]+, named by [^\\s]+/[^\\s]+$"
  },
  {
    "kind": "cut",
    "states": [
      "search",
      "context",
      "community",
      "processes",
      "unavailable",
      "callers",
      "caller-rows",
      "callees",
      "callee-rows",
      "consumer-rows",
      "contracts",
      "sides",
      "details",
      "choice",
      "trace",
      "trace-rows"
    ],
    "pattern": "^… cut after (?P<shown>[0-9]+) of (?P<total>[0-9]+) lines \\(budget [0-9]+ tokens\\); raise budget for more$",
    "next": "done"
  }
]
```
