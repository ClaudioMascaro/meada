# Test files and fixtures

Detail behind the [architecture map](../architecture.md), which keeps the flow, identities,
seams, invariants and Known ceilings. The code wins over this file, and a change to these
modules updates it.

- Test files by subject: `conformance` (pack), `index` (export, schema rebuild, writes only in
  `.meada/`), `query` (search/context), `workspace` (peers, status, config errors), `gateway`
  (gateway prefixes), `urls` (constants, wrappers, Express, inferred links), `trace`, `decisions`
  (replay, parse errors), `stale` (stale and waiting decisions in `status` and `export`), `decide`
  (the binary in human mode over scripted stdin), `decider` (decider mode against the stub),
  `freshness` (cold vs refreshed, also an edited zod schema dropping a field), `items` (open items and their `search` goldens, fixtures
  `open-few` and `open-generated`), `cli` (the binary under pt_BR, en_US and de_DE `LANG`), `mcp`
  (the binary over stdio), `resources` (resource `search`, `context`, `trace` start and `status`
  over the chain), `channels` (the `channels-api`, `channels-worker`, `channels-billing`
  fixture: kafkajs forms, fan-out, same-repository links, an unrelated `send`; amqplib queues,
  publishes through a binding of another repository, the empty exchange, queues named like the
  exchange and like an unbound key that no publish reaches, and a Kafka topic that shares a
  queue's name; the `channels-queues-api` and `channels-queues-worker` fixtures: SQS by queue
  name and NATS wildcards at `inferred`, also in `context` on a subject and on a pattern),
  `channel_queries` (`search`, `context` and `trace` on channels over the first fixture),
  `channels_jobs` (the `channels-jobs-api` and `channels-jobs-worker` fixtures: BullMQ queues,
  a `new Queue(name)` wrapper, and Redis streams in both client styles),
  `drift` (shape drift over `drift_chain`: the web export with every consumer shape and
  link drift, and `trace` up from `GET /api/orders` with `drift: missing` on the drifting
  consumers' lines only; an edited client wrapper changing its callers' drift like a cold
  chain is in `freshness`; the overlay's gateway Express routes and catalog Next.js routes
  return object literals, so the gateway and catalog exports carry shapes with source
  `literals`, `trace` up from those routes shows drift and not-checked reads, and `context`
  names the source, and a Next.js route answering through a helper and an Express one that may
  redirect are open at the root; the limits of `literals.rs` are unit tests there; the catalog's
  zod routes (`app/api/comments`) give shapes with source `zod`, `trace` up from them (golden
  `chain-drift.trace-zod.txt`) shows the drift of the web reads, and a document wins over a zod
  schema, which wins over literals; the reading rules and limits of `zod.rs` are unit tests
  there; the catalog's `app/api/batches` route and the web's `src/batches.ts` consumers
  (golden `chain-drift.trace-batches.txt`) destructure, iterate and map over the response,
  with the hidden names and the values handed to a function that are not followed; the
  catalog's `app/api/profiles` route and the web's `src/declared.ts` consumers (golden
  `chain-drift.trace-declared.txt`) read declared types, with the optional fields, the package
  type and the freshness of an edited shared type in `freshness`, the limits being unit tests in
  `declared.rs` and `lang/ts/types.rs`; the catalog's shared package and the routes and
  consumers importing it (golden `chain-drift.trace-shared.txt`) cross repositories through
  package imports, with an edited package and a package that gets its name back in
  `freshness`, the views, the levels and the limits (`PACKAGE_HOPS`, `GRAFT_BUDGET`) being unit
  tests in `shape.rs`, and the surface, `kept` and `export *` collisions in `package.rs`; the rules
  and the pattern depth limit are unit tests in `lang/ts/uses.rs`; `context` on the documented,
  zod, literals and drift-free routes (goldens `chain-drift.context-route-*.txt`), `search` with
  the kind `drift` (`chain-drift.search-drift.txt`) and `status` (`chain-drift.status.txt`, and
  `cli.status-drift.*` in `cli` for pt-BR and English) show it),
  `documents` (provider documents over `documented_chain`: export, `trace` to "documented
  only", `search` (routes and kind `document`), `status`, listed documents, `context` on the
  merged route with its documented fields, `context` on a route (documented only and merged);
  inline repositories written by the test (`repo`, `client`, `openapi` helpers): each document
  giving its own gateway forms under `[serve] prefix` and `strip`, a `[serve]` table with only
  `documents` and an unlisted gitignored document left out, a listed path through a symbolic
  link left unread; over
  the `documents-shapes` fixture, a repository with no code whose OpenAPI 3.0 JSON document
  has local `$ref`s into `components`, `allOf`, `oneOf`, an array response,
  `additionalProperties: true`, a cycle and an external `$ref`, plus a later
  `v2.openapi.json` listing one route again: the export with every shape and `status` with
  the external reference; freshness of an edited document and of a dropped `[serve]` is in
  `freshness`), `asyncapi` (AsyncAPI documents over `asyncapi-events`, a repository with a
  `[serve]` table whose `asyncapi.yaml` (2.6, `kafka-secure`), `billing.asyncapi.yaml` (3.0,
  `amqp`), `sensors.asyncapi.yml` (`mqtt`), `routing.asyncapi.yaml` (a server without
  `protocol`, a channel naming a server the document lacks), `serverless.asyncapi.yaml` (no root
  `servers`) and `legacy.asyncapi.json` (1.2) sit beside two
  code sides that merge with a documented one, and `asyncapi-services`, which names it as a
  peer and holds the code on the other side of each documented one: both exports with the
  channel contracts, shapes and links, the merge of a documented and a code side, `status`
  (protocol, parameterized channel, version), `trace` down to a documented consumer and up
  from a handler to a documented producer, `context` on documented channels with their payload
  fields, and `search` of channels and of documents). Table tests for discovery, format detection, routes, response selection, the
  schema-to-shape reader (`schema.rs`) and the YAML reader are unit tests in `src/document/`,
  with the hostile inputs each limit stops (server variables naming each other, 200,000 nested
  block sequences, a 10,000-long reference chain, one wide schema read for many routes).
- `tests/packages.rs` (spec #22): the package fixtures `pkg-{shared,web,api,mono,fork}` as
  siblings (`web`, `api` and `mono` name `shared` as a peer, `fork` names `mono`), through
  `Engine::export`. `shared` holds `@example/shared` and an ignored-by-rule
  `node_modules/left-pad/package.json`; `web` and `api` depend on it (`api`'s manifest has no
  name and a non-string range; `web` accepts the `shared` version, `api` does not (goldens carry
  `range mismatch` and `range unchecked`); `web` also names `express` and `left-pad`, outside the workspace);
  `mono` has `packages/{a,b,c}` (`b` on `a` through `workspace:*`, `c` on `a` with `^1.0.0`) and a
  `.gitignore`d `ignored/`, and `fork` holds another `@example/a`. Goldens `pkg-*.export.json`;
  a freshness test edits manifests in three repositories (`shared`, `web`, `mono`), and renames
  an export and an `exports` subpath of the kit, and compares the exports of all four with a cold
  set; a test shows an ambiguous package link is no open item. Imports (spec #22): `shared` also
  holds `kit/`, `@example/kit`, whose `exports` (conditions, a `./utils/*` pattern, a `null`
  `./internal/*`) and `main` point straight to sources, with a re-export, an `export *` cycle, a
  default and a class; `web/src/app.ts` calls it through a named, default, aliased, namespace,
  pattern and member import, plus a blocked subpath, an unexported name, an unlisted subpath,
  `express` and a name never imported (none cross), and the hidden names probe (a parameter, a
  `catch` binding, a destructuring and a `let` named like the import: no hop); `api/src/report.js`
  calls `greet` too; `mono`'s `a` has `main`, called from `b` (`workspace:*`, exact) and `c`
  (ambiguous with `fork`). `shared` also holds `@example/typed` (`types` to a source) and
  `@example/esm` (`module` to a `.mjs`, and subpaths read as paths, one through `index`), which
  `web/src/entries.ts` calls; the root package `@example/shared` ships build output, with an `exports`
  map into `dist` (conditions, a `./utils/*` pattern), a `tsconfig.json` (`outDir`, `rootDir`) and
  `src/`, which `web/src/text.ts` and `api/src/label.js` call (`inferred`, `import outDir`), and
  `@example/sealed` (`main` into `dist`, a `tsconfig.json` with no `rootDir`, two files exporting
  `seal`) is `ambiguous` with both listed; `web/e2e/` declares the kit in `devDependencies` and crosses, and
  `web/tools/`, whose own manifest does not declare the kit though the root one does, does not.
  `shared` also holds `@example/dts` (`types` to a declaration file that is no source, `main` to
  `index.js`: `exact`, `import main`), `@example/plain` (no entry field, an `index.ts`: `exact`,
  `path`) and `@example/bare` (no entry field and no `index`, one file exporting the name:
  `ambiguous`, `path`), called from `web/src/entries.ts`; `@example/prec` (`types` into `dist`,
  mapped through its own `tsconfig.json`, and a later `main` that is a source: the first field
  wins, `inferred`) and `@example/nest` (no entry field, with `index/` and `sub/` nested
  packages: `index/index.ts` is not its default and a subpath into `sub/` does not cross),
  called from `web/src/nest.ts`. The freshness test also asserts the calls through the renamed export and subpath no longer
  cross. Hidden names in the trunk: `conformance-ts-js/src/shadow.ts` (`catch`, `for ... of`,
  `let`, `var`, an arrow parameter without parentheses, nested and array parameter patterns, a
  same-scope constant and parameter, each never `exact`) and two value-flow cases in
  `http-fetch-param` (a `for ... of` hiding a constant URL, a `catch` hiding a parameter that
  defaults to `fetch`). Goldens `pkg-web.trace-down.txt` (from the call into `greet` and on to
  `shout`) and `pkg-shared.trace-up.txt` (to `web` and `api`). `search` and `context` (ticket
  T5): `pkg.search-package*.txt` (kind `package`, all and by name), `pkg.search-contract-shared.txt`
  (dependency entries placed at their manifest and field), `pkg-shared.context-greet.txt` (the
  `consumers:` of the kit's `greet`) and `pkg-shared.context-greet-flagged.txt` (the same after
  editing two ranges to `^3.0.0` and `latest`: `[range mismatch]`, `[range unchecked]`),
  `pkg.context-package-kit.txt` and `pkg.trace-package-kit.txt` (a package contract as a
  `context` subject, and as a `trace` start), and `pkg-mono.context-add.txt` (the consumers of
  `mono`'s `add`, both in its own repository).
  Ticket T4 adds `pkg-web.trace-down-outdir.txt` and `pkg-shared.trace-up-outdir.txt` (the calls
  through `@example/shared`); an invalid, hostile or edited nested `tsconfig.json` is tested the
  same way as a `package.json` (refreshed equals cold).
- **The loopback decider stub** (`tests/common/stub.rs`): `Stub::start(script: Vec<Reply>)`
  listens on `127.0.0.1:<free port>` (`Stub::url`, the value for `MEADA_DECIDER_URL`), answers
  the n-th request to `.../chat/completions` or `.../api/alpha/decisions` with `script[n]` (500
  once it runs out, 404 on other paths) and records every request it receives
  (`requests() -> Vec<Request {method, path, headers, body}>`, `header(name)`, `json()`).
  `Reply::chat(model, content)`, `Reply::ok(json)`, `Reply::status(code, body)`.
  `tests/decider.rs` runs the binary with only the decider variables it sets and asserts the
  key is in no output; `--show-request`, `--list`, human mode, `index`, `export`, `status` and
  the MCP server leave the stub with zero requests. Answer cases (malformed, threshold,
  `--min-p`, `--dry-run`) script the stub per test; a malformed first answer must leave the
  second item written.
