# Test crates, harness and fixtures

- `meada-core/tests/` tests that crate at its public seam, and `cargo test` runs them with the
  binary's (both crates are default members): `ids.rs`, the bundle and generation ids against
  their canonical texts (the cases the `snapshot` module held), `is_id`, `fence`,
  `entry_blocks`, and the workspace version id (its canonical text, the same for the same
  entries however written, changed by a swap, a path, a block or an owner, an entry without one
  keeping its ownerless text, two owners differing in case giving two ids); `manifest.rs`,
  `validate` (the entries in order, text errors with file, line and dotted key, every
  unservable identity named, each `owner` error and the names it accepts); `workspace.rs`,
  `diff` and `entries_text` (the owner printed after the path); `export.rs`, every `*.export.json` golden read with `export::parse` and written
  again byte for byte, and `parse` refusing a missing field or one of the wrong type, naming it.
  The binary's manifest errors stay in `tests/it/manifest.rs`.
- `tests/it/benchmark.rs` is an ignored development harness over the public chain (with
  document/drift overlays), every conformance fixture and peer, and every cross-language
  chain. It reuses the disk fixture helpers and `Engine`, times cold workspace indexing and
  refresh after one root source edit, verifies the changed file hash through the export,
  and reports each index's logical file size plus root/workspace totals. Fixed default
  search/context/trace queries reject empty or missing subjects. Token costs use
  `meada-core::render::tokens`; the MCP stdio list records both the full compact result and
  descriptions plus schemas, which the existing tests budget. A fixture inventory guard
  catches new conformance/cross-language directories. CI runs the release harness on every
  PR and main push, retaining JSON/Markdown and a job-summary table. Timings are informational.
- `tests/it/common/mod.rs`: `fixture_repo(case, fixture, origin)` copies `tests/fixtures/<fixture>`
  to `$CARGO_TARGET_TMPDIR/<case>/<fixture>` and makes it a git repository (`init`, `origin`,
  one commit). `case` must be unique per test across every subject, since they run in parallel in
  one process. `chain(case)` builds
  the three-repository chain as siblings, plus `chain-worker` (`worker(sibling)`), which reads
  the `postgres` `orders` resource the gateway writes (the catalog reads it at repository
  level) and handles the Kafka topic `orders.created` the gateway's `createOrder` (`PUT
  /api/orders/:id`, which the web's `createOrder` calls) produces through its `publish` wrapper. `loose_chain(case)` adds an unlinked bucket and a `via` naming no symbol to the
  catalog. `git()` pins config, author and dates; `age()` pushes
  mtimes into the past so a later edit always changes them. `decisions(repo, name)` copies
  `tests/fixtures/decisions/<name>.toml` to `<repo>/meada.decisions.toml`; the base chain fixture
  has none. `documented_chain(case)` lays `tests/fixtures/chain-documents/{gateway,web}/` over
  the chain (and copies `generated-openapi.yaml` to the gateway's gitignored
  `generated/openapi.yaml`, which Meada's own git would ignore inside the overlay), then commits:
  the gateway gains `openapi.yaml` (3.1: `GET /api/orders`, merged with the Express route and
  given a response shape with an open part, and
  the documented-only `GET /api/shipments/{id}`; a `servers` URL on `api.example.com` with base
  path `/gw`), `orders.openapi.json` (3.0, the same shipments route again: a duplicate; servers
  with variable defaults `https://{region}.example.com/{version}` and a relative `/relative/base`,
  and one URL with a variable without default, which gives nothing), a Swagger 2.0 `legacy.openapi.yaml` and an invalid
  `broken.openapi.yaml`, `docs/api-contract.yaml` (3.0, `GET /api/carriers`) and the
  gitignored `generated/openapi.yaml` (`GET /api/generated`), both listed, with a path that does
  not exist, in the gateway's `[serve] documents` (the helper writes that line); the web app gains `src/shipments.ts` (the shipments route through
  `GATEWAY_URL` and through the gateway's host, plus one call per server case) and a vendored `vendor/openapi.yaml`, ignored
  since it has no `[serve]`. Later spec #21 tickets extend this overlay, so the base chain
  goldens never change. `drift_chain(case)`
  (spec #23) lays `tests/fixtures/chain-drift/web/` over the documented chain and commits:
  `src/drift.ts` holds consumers of the documented `GET /api/orders` reading a field the
  document lacks, a present one, one under its open `[].product`, one through the client
  wrapper `src/api/body.ts` (`get(path)`, which returns the parsed body), and one whose name a
  `catch` rebinds (no reads), plus a read below a documented scalar (`[].id.length`), a wrapper
  `load` fetching two routes whose two consumers share their caller's site, and a wrapper `both`
  calling `get` twice, which reaches one consumer site by two paths. Declared types (#64) add
  the catalog route `app/api/profiles/route.ts` (a zod page of profiles) and, in the web app,
  `src/declared.ts` (casts, type arguments of the wrapper `src/api/typed.ts` and of
  `res.json<T>()`, an annotation, `z.infer`, a package type and an optional field),
  `src/api/types.ts` (an interface another file extends) and the golden
  `chain-drift.trace-declared.txt`; the catalog route `app/api/labels/route.ts` returns a literal
  typed by an index signature, and the web's `src/labels.ts` reads under that open part (golden
  `chain-drift.trace-labels.txt`). Shapes through packages (#175) add the catalog's package
  `packages/contracts/` (`@chain/contracts`: an entry `src/index.ts` re-exporting by name, with
  `export *` and as types, plus a default zod schema; `quote.ts`, `shelf.ts` and `receipt.ts`
  hold a zod schema, constants, an interface, a union and a type with an index signature), a
  root `package.json` in the catalog, the gateway and the web app depending on it, the catalog
  routes `app/api/quotes`, `app/api/tickets` (the default export) and `app/api/offers` (a
  namespace import) parsed with the shared schemas, `app/api/tags` (a package return type)
  and `app/api/receipts`, the gateway's `src/shelves.ts` (spreading the shared
  constants) and the web's `src/shared.ts` (casts, an annotation, a type argument and `z.infer`
  of the package's names; golden `chain-drift.trace-shared.txt`). Error answers in other forms
  (the gateway's `GET /api/inventory/:sku/price`, the catalog's `app/api/reviews`), a `GET
  /api/ping` both serve (ambiguous) and the web's `src/open.ts` (a dynamic URL the decisions file
  `chain-drift-web` links) cover the rest of the drift rules. Later spec
  #23 tickets extend this overlay, so the `chain` and
  `chain-documents` goldens never change.
- `scripts/edges <dir>[=<origin url>] [<peer dir>...]` is the repro harness for reviewing a
  resolution or contract change: it copies the directories as sibling repositories, exports the
  first and lists each call's level and target, its providers, consumers and links (needs `jq`).
- `scripts/regexp-oracle.go` is Go's side of the `alert::regexp::refusal` check (#718): `cargo test
  --lib alert::regexp::tests::refusal_agrees_with_go -- --ignored --nocapture` feeds a generated
  corpus through it (needs `go`, no network) and fails on a pattern `refusal` judges refused that
  Go compiles; it reports, per Go error code, the Go-refused patterns left unjudged. Run it after
  a change to `refusal` or `regex_source`.

Reviewing a change to an `*.export.json` golden: `cargo run --example golden_diff` (no argument
diffs every export golden changed in `origin/main...HEAD`; see `examples/golden_diff.rs` for the
`--range` forms) prints added, removed and moved entries per section instead of the raw JSON.

Run the public benchmark with `cargo test --locked --release --test it
benchmark::records_public_fixture_benchmark -- --ignored --exact --nocapture` (one shell
command). `MEADA_BENCH_OUT` selects the report directory; otherwise it is
`$CARGO_TARGET_TMPDIR/public-fixture-benchmark`. Under local `bounded`, prefix Cargo with
`env -u LD_PRELOAD` to measure real fsync, as CI does. Cold means indexes absent, not an
emptied OS page cache; setup and report I/O are outside the timers. The reindex timer covers
the whole workspace after one root source edit. Sizes are logical `index.db` bytes, with
root and workspace totals distinguished. JSON names the fixed subjects, edited sources and
all repository fixtures; it contains no private corpus data. Existing tests own token limits.

- `ui/tests/operations.test.ts` projects operational API fixtures into fleet diagnostics,
  retention/bundle metadata, repository provenance, cached credential lifetime, token-check
  outcomes and the admission page's fields, the identity provider's type beside the issuer. `operations.test.tsx` exercises the five pages through their DOM and HTTP client,
  including bilingual accessible controls and the pasted token's lifetime.
- `ui/tests/actions.test.ts` checks the operator/operations gates, eligible generation actions,
  shell-quoted commands and exact u64 recovery floors. `action-pages.test.tsx` exercises those
  actions through the DOM and HTTP client: no write before confirmation or after Escape,
  named holder refusals, successful writes followed by fleet reload, missing bootstrap reads,
  and bilingual keyboard controls. `ConfirmDialog.test.tsx` checks focus restoration,
  cancellation and the busy state through the native dialog's DOM boundary.
- `scripts/test-services up|down` starts the `cloud` job's Valkey, S3 and Postgres containers on loopback and starts the TLS Postgres too and prints the `MEADA_TEST_*` exports, `MEADA_TEST_SERVICES_REQUIRED=1` included; `tests/it/test_services.rs` fails when its images, ports or credentials differ from `ci.yml`'s.
- `scripts/compose-check` checks the local, generations and explicit console profiles without
  starting them, including parsed bootstraps, independent console storage and proxy admission.
  `scripts/helm-check` covers default and enabled console resources, optional Ingress, private
  CA and OIDC values, negative module/operation combinations and managed refresh ordering. It
  also renders `values-accounts.yaml` (#565): the `[accounts]` sections of the console, the nodes
  and the refresh job, the private key mounted in the console's container alone (and in no other
  pod spec), the public key and admin password mounts, groups and GitHub from values, break glass
  and the chart's refusals.
