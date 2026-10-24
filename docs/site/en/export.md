# JSON export

`meada export` prints the canonical JSON for **one repository**: its index, links touching
its consumers/providers and processes of its entry points. Processes can cross peers.
To select a repository in server mode, use
`meada export --workspace meada.workspace.toml --repo example.com/acme/api`;
see [Commands](cli.md) for `--generations` and required features.

Output is pretty-printed with a final newline. It contains no source text or environment
variable values. There is no format-version field: identify the producer by the package/binary
version. Readers should tolerate additional fields. Shared types live in
[`meada-core/src/export.rs`](https://github.com/ClaudioMascaro/meada/blob/main/meada-core/src/export.rs)
and [`shape.rs`](https://github.com/ClaudioMascaro/meada/blob/main/meada-core/src/shape.rs).

## Root object

`T[]` means a list of records of type `T`; `string[]` is a list of strings. The type names
below explain objects; they are not extra JSON keys.

| Field | Type | Emission |
| --- | --- | --- |
| `repository` | string | Always: repository identity. |
| `files` | `File[]` | Always, including `[]`. |
| `symbols` | `Symbol[]` | Always, including `[]`. |
| `calls` | `CallSite[]` | Always, including `[]`. |
| `providers` | `Provider[]` | Always, including `[]`. |
| `consumers` | `Consumer[]` | Always, including `[]`. |
| `links` | `Link[]` | Always, including `[]`. |
| `bindings` | `Binding[]` | Omitted when empty. |
| `documents` | `Document[]` | Omitted when empty. |
| `publishes` | `Publish[]` | Omitted when empty. |
| `packages` | `Package[]` | Omitted when empty. |
| `dependencies` | `Dependency[]` | Omitted when empty. |
| `imports` | `Import[]` | Omitted when empty. |
| `communities` | `Community[]` | Omitted when empty. |
| `processes` | `Process[]` | Omitted when empty. |
| `stale_decisions` | `Stale[]` | Omitted when empty. |

Omitted lists are read as `[]`. A repository without records can be represented by:

```json
{
  "repository": "example.com/acme/web",
  "files": [],
  "symbols": [],
  "calls": [],
  "providers": [],
  "consumers": [],
  "links": []
}
```

## Files, symbols and calls

| Record | Always emitted fields | Omitted fields |
| --- | --- | --- |
| `File` | `path: string`, relative path; `hash: string`, BLAKE3 of the file bytes, 64 lowercase hexadecimal digits. | None. |
| `Span` | `path: string`; `start: integer`; `end: integer` (stored as signed 64-bit integers). | None. |
| `Symbol` | `id: string`, symbol identity; `span: Span`. | None. |
| `CallSite` | `id: string`, call identity; `caller: string`, its enclosing caller; `target: string or null`, resolved symbol; `level: Level or null`; `span: Span`. | `candidates: string[]` when empty. |

**`Span.start` and `Span.end` are zero-based UTF-8 byte offsets, with an exclusive end**,
not line numbers or character positions. The span is `bytes[start..end]` in `path`; check
the file hash before using positions from an old export. MCP display lines are derived
from these offsets.

A symbol identity combines `<repository>::<path>::<qualified-name>`; a call adds its call-site
identity, such as `example.com/acme/web::src/orders.ts::load::fetch#0`.
Paths are relative to the repository.

## Contracts and packages

| Record | Always emitted fields | Fields omitted when absent |
| --- | --- | --- |
| `Provider` | `contract: string`; `site: string`, provider side. | `gateway: string`, rewritten path; `shape: Shape`. |
| `Consumer` | `contract: string or null`; `site: string`, consumer side; `via: string or null`, such as `host:api.example.com` or `env:API_URL`. | `shape: Shape`. |
| `Binding` | `contract: string`, queue channel; `exchange: string`; `pattern: string`, routing pattern; `site: string`, binding call. | None. |
| `Publish` | `exchange: string`; `key: string`, routing key; `site: string`, publish call. | None. |
| `Document` | `path: string`; `format: Format`; `version: string or null`, document's declared version; `contracts: string[]`, unique and sorted. | None. |
| `Package` | `name: string`; `repository: string`; `directory: string`, manifest directory relative to the root (`.` at the root). | `version: string`; `ecosystem: string`, omitted for npm, currently `pypi`, `cargo` or `go` otherwise. |
| `Dependency` | `manifest: string`, path; `field: string`, manifest key; `name: string`, package name. | `alias: string`, Cargo's renamed key; `range: string`, requirement as written in the manifest. |
| `Import` | `site: string`, call through the imported name; `specifier: string`, imported module; `name: string`, name used; `target: string`, reached symbol; `level: Level`; `how: How`. | None. |
| `Link` | `consumer: string`; `provider: string`; `contract: string`; `level: Level`; `reason: string`. | `by: string`, human/model for a decided link; `p: number`, model's stated probability; `drift: Drift`. |

`site`, `consumer` and `provider` may identify a call, symbol, file, manifest or whole repository,
depending on the contract kind. A documented-only side also has an identity, for example
`<repository>::openapi.yaml::GET /orders/{}`. Contract identities include
`http::GET::/orders/{}`, `resource::postgres::orders`, `channel::kafka::orders.created` and
`package::npm::@example/shared`.

An unresolved HTTP call has `Consumer.contract: null`. An unresolved channel side can have
`channel::<broker>` without a name; it is shown but never linked. Nullable `target`, `level`,
`contract`, `via` and `Document.version` fields in the tables are emitted as `null`, rather
than omitted. They differ from optional fields in the last column.

## Communities, processes and stale decisions

| Record | Always emitted fields | Omitted fields |
| --- | --- | --- |
| `Community` | `name: string`, local community name; `members: string[]`, sorted member identities. | None. |
| `Process` | `entry_point: string`, entry-point identity; `kind: EntryKind`; `label: string`; `steps: Step[]`, depth-first steps. | None. |
| `Step` | `depth: integer`, unsigned; `repository: string`; `level: Level or null`, level of the step that reached it; `via: Via`; `node: string`, node identity or the call a note refers to. | `reason: string`, `mark: Mark`, `contract: string` when absent; `candidates: string[]` when empty. |
| `Stale` | `call: string`; `target: string`; `cause: Cause`. | None. |

`Step.level` is emitted even when `null`. Cycle, seen-node and ambiguity marks explain why a
branch stops. A decision waiting for a missing peer is not stale and does not enter
`stale_decisions`; see [Decisions](decisions.md).

## Shapes and drift

| `Shape` field | Type and meaning | Emission |
| --- | --- | --- |
| `source` | `Source`, described below. | Always. |
| `fields` | `string[]`, present field paths. | Always, including `[]`. |
| `open` | `string[]`, parts whose fields cannot be enumerated. | Omitted when empty. |
| `scalars` | `string[]`, scalar leaves, with nothing below them compared. | Omitted when empty. |
| `declared` | `string[]`, consumer fields supplied by a declared type. | Omitted when empty. |
| `optional` | `string[]`, optional type fields that a read has not made required. | Omitted when empty. |
| `guessed` | `string[]`, provider parts from a package import resolved below `exact`. | Omitted when empty. |

All path lists are unique and sorted. Paths use dots and `[]`: `items`, `items[]`,
`items[].id`; an array payload starts with `[].id`; `""` denotes the whole root for open/scalar
parts. Values are not exported and types are not compared. `Shape.imports` belongs to internal
storage: queries fill it and remove it before export. It **is not a canonical export field**;
do not confuse it with root `imports`, which contains public `Import` records.

`Source` is an object with **one** key and a string path: `{"document":"openapi.yaml"}`,
`{"zod":"src/orders.ts"}`, `{"literals":"src/orders.ts"}` or `{"reads":"src/orders.ts"}`.
It is neither a string nor a `type/path` pair. Provider precedence is document, then zod,
then literals; `reads` describes consumer reads.

`Drift` has two optional maps, omitted when empty (read as `{}`):

- `missing: { path: Level }`: first missing field on each branch. Currently `exact` when
  both sides are declared and the provider part was not guessed, otherwise `inferred`.
  This diagnostic level is separate from the link's level.
- `unchecked: { consumerPath: openProviderPath }`: strings for unchecked parts, which never
  count as drift. The open path can be `""` for the root.

Map keys are sorted. Absent optional fields do not produce `missing`; a `guessed` part caps
its diagnostic at `inferred`. A link may contain only `unchecked`; when all reads are checked
and present, `drift` is omitted.

## JSON enum values

| Type | Exact values |
| --- | --- |
| `Level` | `exact`, `declared`, `inferred`, `decided`, `ambiguous` |
| `Format` | `openapi`, `asyncapi` |
| `How` | `exports`, `types`, `module`, `main`, `path`, `outDir`, `moduleName`, `topLevel`, `crateName`, `modulePath` |
| `EntryKind` | `route`, `export`, `channel`, `program` |
| `Via` | `entry`, `call`, `link` |
| `Mark` | `cycle`, `seen`, `ambiguous`, `unresolved`, `no-provider`, `produces`, `consumes`, `depth-limit` |
| `Cause` | `call-gone`, `item-not-open`, `target-gone`, `not-candidate` |

`How` uses camelCase in JSON even when text output says `module name` or `crate name`.
`Cause` distinguishes removed call, resolved/not-open item, removed target and target outside
the candidates, respectively.

## Deterministic ordering

The same workspace produces the same bytes. Ordering is semantic, not alphabetical across
the whole JSON: files by path; symbols/calls by identity and span; providers by contract/site;
consumers by site/contract/via; bindings by contract/exchange/pattern/site; publishes by
exchange/key/site; documents by path; packages by name/directory; dependencies by
manifest/field/name/alias; imports by site/target; communities by name and members; links by
consumer/provider; processes by entry point with depth-first steps. See complete examples in
the [public goldens](https://github.com/ClaudioMascaro/meada/tree/main/tests/golden).
