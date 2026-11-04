# Package, channel and gateway fixtures

- `tests/it/packages.rs` (spec #22): the package fixtures `pkg-{shared,web,api,mono,fork}` as
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
- `tests/it/pypi.rs` (spec #24): the PyPI fixtures `pkg-pylib` and `pkg-pyweb` as siblings
  (`pyweb` names `pylib` as its peer). `pylib` provides `Shared_Lib` (`shared-lib`, PEP 621, a
  `src/` layout: `shared_lib/__init__.py` re-exports `greet` from `core.py`, which calls
  `text.py`'s `shout`; `api.py` holds `fetch` and `Client.create`; `src/sl_extras/` is a second
  top-level module; `src/shared_lib/requirements.txt` takes none of its files),
  `tools/pyproject.toml` provides `shared.tools` (`shared-tools`, Poetry) whose modules are
  `tools/sl_extras/` and `tools/toolkit/`, and `js/package.json` is an npm `shared-lib`. `pyweb`
  depends on `shared-lib` in `pyproject.toml` (extras and markers) and `requirements-dev.txt`, on
  `shared-tools` in `requirements.txt` (with option lines, a comment and a URL) and in
  `worker/pyproject.toml` (Poetry, a dev group naming `shared-lib` not read), and its
  `ui/package.json` names `shared-tools` on npm (no link). `app/main.py` crosses `exact` through
  the re-export, a submodule reached as `shared_lib.api.fetch` and `from shared_lib.api import
  Client`, `ambiguous` through `sl_extras` (both distributions expose it), and not at all
  through a local `greet` or `json`; `app/rel.py`'s relative imports named after the
  distribution never cross; `tests/test_api.py` crosses `exact` through the root's manifests,
  though `tests/requirements.txt` names only `pytest`; `worker/job.py` crosses `inferred`
  through `toolkit`. Goldens `pkg-{pyweb,pylib}.export.json`, `pkg-pylib.context-greet.txt`,
  `pkg-pylib.search-package.txt` (the PyPI and the npm `shared-lib` apart) and
  `pkg-pyweb.trace-down.txt`; the freshness test edits both repositories' `pyproject.toml`, the
  root `requirements.txt` and adds `worker/requirements.txt`. The fixture `pkg-uv-sources` (#533)
  holds two `bird-feeder` packages under two uv workspace roots and consumers that choose with
  `workspace = true` and `path`, each `exact` to its own, and one with no source, `ambiguous` to
  both (golden `pkg-uv-sources.export.json`; its `stub`, `registry` and `local` (#535) hold a lone `anyio` of the
  repository: the `registry` consumer with no source (in its `pyproject.toml` and its `requirements.txt`) links it `inferred`, `local` with a `path`
  `exact`); with the peer `pkg-uv-feeder`, a `git` source keeps
  the repository of its URL and a `url` or `index` source links nothing. Its `pinned/requirements*.txt` pin
  `bird-feeder==24.4.2` and `>=1,<2` against local `1.0.0` and `2.0.0` (`range mismatch` for the
  pin, and for `>=1,<2` against `2.0.0` alone, #534). Unit tests in `package/pypi.rs` (PEP 503
  names, requirement lines and continuations, both readers, the limits, uv sources, workspace
  members and `inherit`, `flag`) and `package/pep440.rs` (version order, each operator, `~=`,
  wildcards, pre-releases, local labels, `===`, unreadable specifiers), and
  `lang/python/pyproject.rs`.
- `tests/it/cargo.rs` (spec #30): the Cargo fixtures `cargo-lib` and `cargo-web` as siblings
  (`web` names `lib` as its peer). `lib` is a workspace of `shared-core` (`pub use text::{greet,
  shout}` of a private module, `pub mod api` with `Client::create`), `shared-extras` and
  `inner-app`, which depends on both by path. `web` depends on `shared-core` by version (`app`),
  through `workspace = true` and a renamed key (`worker`), on `shared-extras` by a renamed `git`
  key, on a `path` that lands in `lib` (`../../cargo-lib/extras`, linked) and one that lands in
  no workspace repository (`ghost`, not linked), and on `serde` (no link). The tests check each
  form's link and reason, every `use` into `lib` (`exact`, `; import crate name`), the path
  dependency that stays inside `lib`, a `git` URL of another repository that links to nothing,
  a private module that is not reached until it is made `pub`, a `[lib] name` and a
  `version.workspace = true` written into copies of `lib`, and the `cargo-fork` fixture (a second
  provider of `shared-core`, added as a peer) that makes the version dependency `ambiguous`.
  Goldens `cargo-{web,lib}.export.json`, `cargo-lib.context-greet.txt` and
  `cargo-web.trace-down.txt`; the freshness test edits the library's version, `app`'s
  requirement, the workspace requirement and a dependency key. Unit tests in `package/cargo.rs`
  (the reader, unreadable sources, the dedup, `workspace = true`, hostile manifests) and
  `package.rs` (`reaches`).
- `tests/it/gopkg.rs` (spec #25): the Go fixtures `gopkg-lib` (modules `kit`, with `text` and `api`
  packages and a `cmd/app`, and a nested `extras`) and `gopkg-web` as siblings (`web` names `lib`
  as its peer; its nested `worker` module requires `extras` with a `replace` to
  `../../gopkg-lib/extras`), and `gopkg-fork` (a second provider of `kit`, added as a peer, which
  makes the `require` and the import `ambiguous`). The tests check each `require` link and reason,
  every import into `lib` (`exact`, `; import module path`), a directory the provider lacks, a
  module outside the workspace, a local that hides the import, a name only a test file, a file built for another target or two files of the directory define, a directory of a nested module, a `replace` by another module (no link when no repository holds it; with `gopkg-replace`, `gopkg-replace-orig` and `gopkg-replace-fork`, which declare one module path, an `exact` link to the fork alone, `ambiguous` to both without the `replace`, and none when the fork declares another path; golden `gopkg-replace.export.json`), a module path with no dot, and the freshness of
  an edited `go.mod`. Goldens `gopkg-{web,lib}.export.json`, `gopkg-lib.context-greet.txt` and
  `gopkg-web.trace-down.txt`. Unit tests in `package/go.rs` and `lang/go/modfile.rs`.

- `tests/it/jvmpkg.rs` (specs #26, tickets #93 and #94): the Maven and Gradle fixtures `jvm-maven`
  (modules `models` with `Order`, `Repo extends Base`, `Event` with an opaque base, `Note` with
  a method named after it, `Ledger`, a second `example.shared.Stamp` and a test-only
  `Fixtures`), `jvm-gradle` (`kit` with a second `Ledger` in another package, and `tools` whose
  `main` and `test` source sets are literal directories), `jvm-client` and `jvm-gradle-client`
  as the dependents. Manifest links (build files) and imports into provider classes (exact,
  ambiguous by provider, by type and by overload, overloads merged over the provider-side
  family, `inferred` beside an opaque base, constructors, on-demand and static imports, two
  on-demand imports that both bring a class, a same-package class and a parameter or local
  that hide the import, test roots and outside types left out); an edited source set
  relinks like a cold workspace; `context` on the class `Order` lists the calls that reach its
  members. A catalog and `gradle.properties` fill a build's holes (#467): accessors with dots
  and dashes, a missing alias, a catalog past the size limit or no TOML, a settings-named
  catalog and one that leaves its directory, the module's property beating the repository's and
  a script assignment beating both. Goldens `jvm-*.export.json`, `jvm-maven.context-total.txt`,
  `jvm-maven.context-order.txt` and `jvm-client.trace-down.txt`. Unit tests in
  `package/maven.rs` and `jvm/gradle.rs`.

- `tests/it/swiftpkg.rs` (spec #29, ticket #124): the fixtures `swift-pkg-kit` (a `Kit` package
  with a library product of two targets, an executable product, internal declarations and
  overloads of `greet`), `swift-pkg-lib` (a `Shared` package also exposing `KitMoney`, and an `Examples` package that depends on its root), `swift-pkg-dyn` (a package whose name is computed) and
  `swift-pkg-app` (a `.package(url:)`, a `.package(path:)`, computed URLs, an outside
  dependency, a `Package.resolved` that must stay unread and names that hide the imported ones). Manifest links, imports narrowed by labels (a bare, a module-qualified and an
  unlabelled call, a constructor, a method on a local and on a typed parameter), the ambiguous
  module, `context` on a public function and method, `trace` down across the hop, and edited
  manifests refreshing like a cold workspace. Goldens `swift-pkg-{kit,lib,app}.export.json`,
  `swift-pkg-kit.context-{greet,hello}.txt`, `swift-pkg-app.trace-down.txt` and
  `swift-packages.search.txt`; `conformance-swift.export.json` gains the `Shop` package. Unit
  tests in `package/swiftpm.rs`, `package/swiftpm/manifest.rs` and `lang/swift/manifest.rs`.
- `tests/it/nugetpkg.rs` (spec #28, ticket #116): the fixtures `dotnet-lib` (a `PackageId`
  project `Example.Kit` with `Order`, and a `Money` project whose `AssemblyName` differs from its
  file) and `dotnet-app` (a `PackageReference` project, a `ProjectReference` one to the other
  repository and to a project of its own, an outside `Newtonsoft.Json`), and `dotnet-twin`
  (a second provider of the same type, for the ambiguous hop). Manifest links, `using` and
  `using static` imports, `context` on the type and a method, `trace` down across the hop, and an
  edited, renamed and added `.csproj` refreshing like a cold workspace. Goldens
  `dotnet-{lib,app}.export.json`, `dotnet-lib.context-{order,total}.txt` and
  `dotnet-app.trace-down.txt`. Unit tests in `package/nuget.rs` and `dotnet/msbuild.rs`.
- C# channels (spec #28, ticket #115): `conformance-csharp` gains `Channels.cs` and
  `Channels.Nats.cs` (a client of each library: Confluent.Kafka, RabbitMQ.Client with a
  default-exchange publish and a named one routed by a binding, AWSSDK.SQS, NATS.Client with a
  wildcard subject, StackExchange.Redis streams, Azure.Messaging.ServiceBus with a queue, a
  topic and two processors on it, and names read from the environment on both sides) and
  `Channels.Writes.cs` (a sender written more than once, one made for a parameter,
  `String.Empty`, `ConfigureAwait`, a consumer-first `BasicConsume` and two handlers on one
  event), checked by
  `csharp` (the definitions and the golden) and `csharp_channel_clients_link_their_producers_to_their_handlers`
  (every link with its level, and the unresolved sides); `xlang-csharp-web` gains `src/events.ts`
  (`announceShipment`, kafkajs) and `xlang-csharp-api` `Messaging/ShipmentListener.cs`, and
  `trace_down_from_a_typescript_producer_reaches_the_csharp_kafka_consumer` holds
  `xlang-csharp.trace-channel-down.txt`. Unit tests in `channel/csharp.rs` (each library, a
  repository type that hides the library's, two namespaces for one name, a name assigned twice, a
  wrapper, the client depth, every write of a sender) and in `lang/csharp.rs` (`await`, `+=`,
  one `name=` per write, a linear pass over thousands of them).
- **The loopback decider stub** (`tests/it/common/stub.rs`): `Stub::start(script: Vec<Reply>)`
  listens on `127.0.0.1:<free port>` (`Stub::url`, the value for `MEADA_DECIDER_URL`), answers
  the n-th request to `.../chat/completions` or `.../api/alpha/decisions` with `script[n]` (500
  once it runs out, 404 on other paths) and records every request it receives
  (`requests() -> Vec<Request {method, path, headers, body}>`, `header(name)`, `json()`).
  `Reply::chat(model, content)`, `Reply::ok(json)`, `Reply::status(code, body)`.
  `tests/it/decider.rs` runs the binary with only the decider variables it sets and asserts the
  key is in no output; `--show-request`, `--list`, human mode, `index`, `export`, `status` and
  the MCP server leave the stub with zero requests. Answer cases (malformed, threshold,
  `--min-p`, `--dry-run`) script the stub per test; a malformed first answer must leave the
  second item written.

### Immutable workspace import and managed status

`generations/imports.rs` uses the binary and real local RustFS: two concurrent imports see
missing through a reply barrier, but only one conditionally creates the immutable document;
the other reports the first author, time and message. A local workspace-pointer swap during
status's informational Valkey probe proves the report and readiness exit code share one
managed workspace read. CLI import is also checked with `generation-pointer` alone.
