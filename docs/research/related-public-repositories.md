# Related public repositories: models for cross-repository fixtures and corpora

- Date: 2026-10-30
- Question: Which sets of public repositories that relate to each other through HTTP, messaging
  channels or packages would make good models for new Meada fixtures and measurement corpora,
  ranked by the gap they fill (package ambiguity first, for #392; exact duplicates last, for
  #517)?
- Outcome: sixteen sets, four verified with Meada 0.1.0; the best real #392 case is a Go fork
  that keeps the original module path (`gogo/protobuf`, `regen-network/protobuf`), whose
  consumer's `replace` today drops the dependency and whose bare `require` fits both providers,
  and `astral-sh/uv`'s own test workspaces add four packages of one name and two likely false
  positives. Section 6 lists the five fixtures to write first and the corpora to keep for #392
  and #517. #392 was closed as out of scope the same day
  (`.out-of-scope/decidable-package-kind.md`), and none of these cases needs a person's
  decision: the gogo consumer names its provider in its `replace`, uv's consumers pick theirs
  with `[tool.uv.sources]` and their workspace's members, and the `black` and `anyio` links are
  likely false positives. They argue for reading more of the manifest, not for reopening #392.

## 1. Method

Candidates came from the relation shapes `docs/site/en/capabilities.md` describes and from the
open questions of #392 and #517, then were checked against the repositories themselves: the
GitHub REST API (`repos`, `commits`, `git/trees`, `contents` at a pinned SHA, and code search to
find consumers of one fork), read-only. Every claim below cites a permalink at the pinned SHA.
File counts are blobs in the recursive tree at that SHA. Licenses come from the API's
`license.spdx_id` or, where it says `NOASSERTION`, from the LICENSE file read at the SHA.

Coverage today (`tests/fixtures/`, 57 directories with a `meada.toml`): every package ecosystem
has a hand-made library and consumer pair (`pkg-*`, `cargo-*`, `gopkg-*`, `jvm-*`,
`kotlin-pkg-*`, `dotnet-*`, `swift-pkg-*`); `pkg-fork`, `cargo-fork`, `gopkg-fork` and
`dotnet-twin` hand-make a provider that serves a name another one serves; `chain-*`,
`gateway-*`, `xlang-*` and `http-*` cover HTTP with `base_url_env`, gateways and nine languages;
`channels-*` and `asyncapi-*` cover messaging; `duplicates-a` and `duplicates-b` cover
`search kind=duplicate`. So the gaps ranked here are real-world shapes, not missing ecosystems.

Three workspaces covering four sets were verified (section 4): each repository shallow-cloned at
its SHA as siblings in a scratch directory, a throwaway `meada.toml` with `[[peer]]` entries
written in the consumer clone, and `meada export` run from it inside
`systemd-run --user --scope -p MemoryMax=2G timeout 900`. Nothing in the clones was built or
run, and the clones were deleted afterwards. Total clone size was about 100 MB.

Clean room: GitNexus's repositories, source and documentation were not read for this note, and
none is a candidate.

## 2. Ranked summary

| # | Set | Repositories | License | Relation kinds | Gap filled | Verified |
| --- | --- | --- | --- | --- | --- | --- |
| 1 | gogo fork | `gogo/protobuf`, `regen-network/protobuf`, `tendermint/cns` | BSD-3-Clause, BSD-3-Clause, Apache-2.0 | Go `require` fitting two providers; `replace` to a fork | #392 trigger: a fork serving the original module path | yes |
| 2 | uv test workspaces | `astral-sh/uv` (one repository) | MIT OR Apache-2.0 (`Apache-2.0` in the API) | PyPI dependency entries fitting 2 to 4 in-repo providers | #392 trigger: a monorepo with several packages of one name, a manifest copy | yes |
| 3 | pubgrub fork | `astral-sh/uv`, `astral-sh/pubgrub`, `pubgrub-rs/pubgrub` | Apache-2.0, MPL-2.0, MPL-2.0 | Cargo `git` entry naming a fork that keeps the crate names | control case: `git` disambiguates a fork | yes |
| 4 | Sphinx namespace | `sphinx-doc/sphinx`, `sphinx-doc/sphinxcontrib-*` | BSD-2-Clause | PyPI entries; one top-level module (`sphinxcontrib`) from six distributions | import-level ambiguity, extras not read | no |
| 5 | tokio stack | `tokio-rs/tokio`, `tokio-rs/mio`, `tokio-rs/bytes`, `tokio-rs/axum` | MIT | Cargo version entries, `optional = true`, non-default features | dependencies used only behind features outside `default` | no |
| 6 | h3 and rou3 | `h3js/h3`, `h3js/rou3` | MIT | npm entry; `exports` to `dist/` with no `outDir` | import crossing when the build output has no tsconfig mapping | no |
| 7 | Pallets | `pallets/flask`, `werkzeug`, `jinja`, `click` | BSD-3-Clause | PyPI entries; imports whose module matches the distribution | baseline corpus for exact PyPI links (none on the dogfood set) | no |
| 8 | spf13 | `spf13/cobra`, `spf13/pflag`, `spf13/viper` | Apache-2.0, BSD-3-Clause, MIT | Go `require`; aliased import; copied functions | baseline Go corpus; near duplicates for #517 | no |
| 9 | Apache Commons | `apache/commons-lang`, `apache/commons-text` | Apache-2.0 | Maven dependency with a property version; varargs overloads | baseline Maven corpus | no |
| 10 | Serilog | `serilog/serilog`, `serilog/serilog-sinks-console` | Apache-2.0 | NuGet `PackageReference`; extension methods; overloads | baseline NuGet corpus, a documented gap | no |
| 11 | SwiftNIO | `apple/swift-nio`, `swift-server/async-http-client`, `apple/swift-log` | Apache-2.0 | SwiftPM `.package(url:)`, library products | baseline SwiftPM corpus | no |
| 12 | Petclinic | `spring-petclinic/spring-petclinic-rest`, `-reactjs`, `-angular` | Apache-2.0, Apache-2.0, none | HTTP Java to TypeScript, OpenAPI; two repositories with one Maven coordinate | URL builder functions; generated controller interfaces | yes |
| 13 | RealWorld | `gothinkster/realworld`, `sveltejs/realworld`, `gothinkster/golang-gin-realworld-example-app` | MIT | HTTP through one OpenAPI document, Go and JavaScript | cross-language HTTP with a document server path | no |
| 14 | OpenTelemetry demo | `open-telemetry/opentelemetry-demo` (services as directories) | Apache-2.0 | Kafka Go producer, C# and Kotlin consumers | channel names from environment with a literal default | no |
| 15 | go-yaml copies | `go-yaml/yaml`, `kubernetes-sigs/yaml` | MIT AND Apache-2.0, MIT AND BSD-3-Clause | 12 byte-identical Go files, 7 changed | #517 ground truth, Go | no |
| 16 | packaging copies | `pypa/packaging`, `pypa/pip`, `pypa/setuptools` | Apache-2.0 OR BSD-2-Clause, MIT, MIT | 22 and 16 byte-identical files, 6 patched | #517 ground truth, Python, three-way | no |

No public pair of separate repositories with a Kafka or RabbitMQ producer in one and its consumer
in the other, let alone with AsyncAPI documents, turned up within the search budget; set 14 is
the closest model and would be split into repositories by the fixture.

## 3. The sets

Permalinks use the pinned SHA. "Copy" means real code may be copied into a fixture with
attribution; "model" means the shape is reproduced in new Apache-2.0 code.

### 3.1 gogo fork (Go, #392)

| Repository | SHA | Files | License |
| --- | --- | --- | --- |
| `gogo/protobuf` (archived upstream) | `f67b8970b736e53dbd7d0a27146c8f1ac52f74e5` | 770 | BSD-3-Clause text (API: `NOASSERTION`) |
| `regen-network/protobuf` (GitHub fork, branch `cosmos`) | `8a70ddfdaf94e68497f7b26a6896ad75e03b6616` | 775 | same |
| `tendermint/cns` | `6ea7237e7c05ded1b4751b729d8cc7b52630a773` | 55 | Apache-2.0 |

Both providers declare the same module path:
[gogo `go.mod#L1`](https://github.com/gogo/protobuf/blob/f67b8970b736e53dbd7d0a27146c8f1ac52f74e5/go.mod#L1)
and [regen `go.mod#L1`](https://github.com/regen-network/protobuf/blob/8a70ddfdaf94e68497f7b26a6896ad75e03b6616/go.mod#L1),
both `module github.com/gogo/protobuf`. The consumer requires a version only the fork has and
replaces the module with the fork:
[`go.mod#L7`](https://github.com/tendermint/cns/blob/6ea7237e7c05ded1b4751b729d8cc7b52630a773/go.mod#L7)
(`github.com/gogo/protobuf v1.3.3`) and
[`go.mod#L21`](https://github.com/tendermint/cns/blob/6ea7237e7c05ded1b4751b729d8cc7b52630a773/go.mod#L21)
(`replace github.com/gogo/protobuf => github.com/regen-network/protobuf v1.3.3-alpha.regen.1`).
Its generated code imports the package and calls into it:
[`cns.pb.go#L10`](https://github.com/tendermint/cns/blob/6ea7237e7c05ded1b4751b729d8cc7b52630a773/x/cns/types/cns.pb.go#L10),
[`#L189`](https://github.com/tendermint/cns/blob/6ea7237e7c05ded1b4751b729d8cc7b52630a773/x/cns/types/cns.pb.go#L189)
(`proto.RegisterType`), defined at
[gogo `proto/properties.go#L545`](https://github.com/gogo/protobuf/blob/f67b8970b736e53dbd7d0a27146c8f1ac52f74e5/proto/properties.go#L545)
and [regen `proto/properties.go#L545`](https://github.com/regen-network/protobuf/blob/8a70ddfdaf94e68497f7b26a6896ad75e03b6616/proto/properties.go#L545).

The shape is common: a code search for `"=> github.com/regen-network/protobuf" filename:go.mod`
reports 2,748 hits (Cosmos chains such as `akash-network/node`, `terra-money/core`,
`fetchai/fetchd`). The successor, `cosmos/gogoproto`, renamed its module to
`github.com/cosmos/gogoproto`, so it serves no name of the original.

What Meada should link (capabilities.md, "Go modules"): `require` links to the workspace provider
of the module path, so the require fits two providers. Go itself takes the `replace` target's
repository whatever its `go.mod` says, so the fork is the provider; capabilities.md says "a
replacement by another module stays out". Section 4.1 shows what that means today.

Suggested fixture: a consumer whose `require` names `example.com/orig/lib` and whose `replace`
names `example.com/fork/lib vX`, beside an `orig` and a `fork` repository that both declare
`module example.com/orig/lib`. It tests the `replace`-to-module rule against a workspace that
holds the replacement, which `gopkg-fork` (a fork with no `replace`) does not. Copy allowed
(BSD-3-Clause, Apache-2.0), but the shape needs a dozen lines and is better modelled.

### 3.2 uv test workspaces (PyPI, #392)

`astral-sh/uv` at tag `0.9.0`, `39b6886536b20a239453b9abce590bbadd99a314`, 1,114 files
(main at `6d74dc11ce84b64b483f6492268460e68901ab4e` has 1,887). Its `scripts/workspaces/` hold
uv's own test workspaces, each declaring packages with the same names:

- `bird-feeder` four times:
  [root-workspace](https://github.com/astral-sh/uv/blob/39b6886536b20a239453b9abce590bbadd99a314/scripts/workspaces/albatross-root-workspace/packages/bird-feeder/pyproject.toml#L2),
  [virtual-workspace](https://github.com/astral-sh/uv/blob/39b6886536b20a239453b9abce590bbadd99a314/scripts/workspaces/albatross-virtual-workspace/packages/bird-feeder/pyproject.toml#L2),
  [in-example](https://github.com/astral-sh/uv/blob/39b6886536b20a239453b9abce590bbadd99a314/scripts/workspaces/albatross-in-example/examples/bird-feeder/pyproject.toml#L2),
  [project-in-excluded](https://github.com/astral-sh/uv/blob/39b6886536b20a239453b9abce590bbadd99a314/scripts/workspaces/albatross-project-in-excluded/excluded/bird-feeder/pyproject.toml#L2).
  `seeds` three times in the same workspaces.
- The consumer says which one it means, in uv's own table:
  [`albatross-root-workspace/pyproject.toml#L5`](https://github.com/astral-sh/uv/blob/39b6886536b20a239453b9abce590bbadd99a314/scripts/workspaces/albatross-root-workspace/pyproject.toml#L5)
  (`dependencies = ["bird-feeder", ...]`),
  [`#L8`](https://github.com/astral-sh/uv/blob/39b6886536b20a239453b9abce590bbadd99a314/scripts/workspaces/albatross-root-workspace/pyproject.toml#L8)
  (`[tool.uv.sources] bird-feeder = { workspace = true }`) and
  [`#L11`](https://github.com/astral-sh/uv/blob/39b6886536b20a239453b9abce590bbadd99a314/scripts/workspaces/albatross-root-workspace/pyproject.toml#L11)
  (`members = ["packages/*"]`), with the import at
  [`src/albatross/__init__.py#L2`](https://github.com/astral-sh/uv/blob/39b6886536b20a239453b9abce590bbadd99a314/scripts/workspaces/albatross-root-workspace/src/albatross/__init__.py#L2).
- A manifest copy named like a real PyPI package:
  [`ecosystem/black/pyproject.toml#L34`](https://github.com/astral-sh/uv/blob/39b6886536b20a239453b9abce590bbadd99a314/ecosystem/black/pyproject.toml#L34)
  (only `pyproject.toml`, `PKG-INFO` and `LICENSE`), and a test package
  [`scripts/packages/black_editable/pyproject.toml#L2`](https://github.com/astral-sh/uv/blob/39b6886536b20a239453b9abce590bbadd99a314/scripts/packages/black_editable/pyproject.toml#L2)
  (version 0.1.0), while the docs pin the real one:
  [`docs/requirements.txt#L11`](https://github.com/astral-sh/uv/blob/39b6886536b20a239453b9abce590bbadd99a314/docs/requirements.txt#L11)
  (`black==24.4.2`).
- A stub that shadows a registry package:
  [`scripts/packages/anyio_local/pyproject.toml#L2`](https://github.com/astral-sh/uv/blob/39b6886536b20a239453b9abce590bbadd99a314/scripts/packages/anyio_local/pyproject.toml#L2)
  (`anyio`, version `4.3.0+foo` at
  [`#L12`](https://github.com/astral-sh/uv/blob/39b6886536b20a239453b9abce590bbadd99a314/scripts/packages/anyio_local/pyproject.toml#L12)),
  while [`scripts/packages/built-by-uv/pyproject.toml#L7`](https://github.com/astral-sh/uv/blob/39b6886536b20a239453b9abce590bbadd99a314/scripts/packages/built-by-uv/pyproject.toml#L7)
  depends on `anyio>=4,<5` with no `[tool.uv.sources]`, so from PyPI.

What Meada should link (capabilities.md, "Python packages"): `[project] dependencies` and
`requirements*.txt` link to workspace packages by PEP 503 name, ranges unchecked, so each of these
entries fits every same-named package. Section 4.2 has the counts.

Suggested fixture: one repository with two packages named alike under two workspace roots, a
consumer that picks one with `[tool.uv.sources] x = { workspace = true }` (and another with
`{ path = "../x" }`), and a `requirements.txt` pin (`x==24.4.2`) that the in-repo `0.1.0` cannot
satisfy. It is the PyPI counterpart of `pkg-mono` and the case #392's decision flow would close.
Copy allowed (MIT OR Apache-2.0, `Cargo.toml` `license`), though the shape is tiny.

### 3.3 pubgrub fork (Cargo, control case)

| Repository | SHA | Files | License |
| --- | --- | --- | --- |
| `astral-sh/uv` (tag `0.9.0`) | `39b6886536b20a239453b9abce590bbadd99a314` | 1,114 | MIT OR Apache-2.0 |
| `astral-sh/pubgrub` (GitHub fork) | `d8efd77673c9a90792da9da31b6c0da7ea8a324b` | about 53 | MPL-2.0 |
| `pubgrub-rs/pubgrub` | `65355b4a1192c5a0fadb81f301e6dea713ae78d8` | 53 | MPL-2.0 |

At the pinned SHAs both repositories are Cargo workspaces of a crate `pubgrub` and a member
`version-ranges`:
[fork `Cargo.toml#L7`](https://github.com/astral-sh/pubgrub/blob/d8efd77673c9a90792da9da31b6c0da7ea8a324b/Cargo.toml#L7),
[upstream `Cargo.toml#L7`](https://github.com/pubgrub-rs/pubgrub/blob/65355b4a1192c5a0fadb81f301e6dea713ae78d8/Cargo.toml#L7),
[fork `version-ranges/Cargo.toml#L2`](https://github.com/astral-sh/pubgrub/blob/d8efd77673c9a90792da9da31b6c0da7ea8a324b/version-ranges/Cargo.toml#L2),
[upstream `version-ranges/Cargo.toml#L2`](https://github.com/pubgrub-rs/pubgrub/blob/65355b4a1192c5a0fadb81f301e6dea713ae78d8/version-ranges/Cargo.toml#L2).
uv names the fork by `git`:
[`Cargo.toml#L148`](https://github.com/astral-sh/uv/blob/39b6886536b20a239453b9abce590bbadd99a314/Cargo.toml#L148),
[`#L198`](https://github.com/astral-sh/uv/blob/39b6886536b20a239453b9abce590bbadd99a314/Cargo.toml#L198),
inherited by members with `workspace = true`
([`crates/uv-resolver/Cargo.toml#L59`](https://github.com/astral-sh/uv/blob/39b6886536b20a239453b9abce590bbadd99a314/crates/uv-resolver/Cargo.toml#L59))
and imported at
[`crates/uv-resolver/src/resolver/mod.rs#L16`](https://github.com/astral-sh/uv/blob/39b6886536b20a239453b9abce590bbadd99a314/crates/uv-resolver/src/resolver/mod.rs#L16).

Since 2026-10-21 the fork publishes as `astral-pubgrub` with `[lib] name = "pubgrub"`
([main `Cargo.toml#L7`](https://github.com/astral-sh/pubgrub/blob/6449055dabbd4f0d6b8387c06746751dd47d1d8c/Cargo.toml#L7),
[`#L28`](https://github.com/astral-sh/pubgrub/blob/6449055dabbd4f0d6b8387c06746751dd47d1d8c/Cargo.toml#L28)),
and uv's main renames it back
([`Cargo.toml#L229`](https://github.com/astral-sh/uv/blob/6d74dc11ce84b64b483f6492268460e68901ab4e/Cargo.toml#L229),
`package = "astral-pubgrub"`). Pinning `0.9.0` keeps the older, harder shape: two providers of
one crate name, told apart only by the `git` URL.

What Meada should link (capabilities.md, "Rust packages"): `git` resolves to the repository of the
normalized URL, `workspace = true` reads `[workspace.dependencies]`, and `use` follows the
dependency key, all `exact`. Verified in section 4.2. A version-only consumer (`pubgrub = "0.3"`)
in the same workspace would fit both; that is the case `cargo-fork` hand-makes.

Suggested fixture: none new for the `git` path (it works); a variant of `cargo-fork` with the
fork renamed and `[lib] name` kept (the main-branch shape) would test that `use pubgrub::` follows
the renamed dependency key rather than any crate whose library is `pubgrub`. Model only
(MPL-2.0 is file-level copyleft, outside the copy list).

### 3.4 Sphinx namespace packages (PyPI)

`sphinx-doc/sphinx` at `035a07c32c3241fd3356f4d827800c1f88525272` (2,059 files, BSD-2-Clause per
`LICENSE.rst`, API `NOASSERTION`); `sphinx-doc/sphinxcontrib-applehelp` at
`f4f9d900b238d03da601bf7c75cff5bcbcee6d7c` (123 files) and five siblings
(`sphinxcontrib-serializinghtml` at `f0f3b15b2e6bb47570d605ebd5687643d7a85b9d`, 131 files, and
`-devhelp`, `-htmlhelp`, `-jsmath`, `-qthelp`), BSD-style `LICENCE.rst`.

- Sphinx depends on all six:
  [`pyproject.toml#L70`](https://github.com/sphinx-doc/sphinx/blob/035a07c32c3241fd3356f4d827800c1f88525272/pyproject.toml#L70)
  to [`#L75`](https://github.com/sphinx-doc/sphinx/blob/035a07c32c3241fd3356f4d827800c1f88525272/pyproject.toml#L75),
  and loads them by name, not by import:
  [`sphinx/application.py#L130`](https://github.com/sphinx-doc/sphinx/blob/035a07c32c3241fd3356f4d827800c1f88525272/sphinx/application.py#L130).
- Each extension is a distribution `sphinxcontrib-<x>` exposing module `sphinxcontrib.<x>`
  ([`pyproject.toml#L7`](https://github.com/sphinx-doc/sphinxcontrib-applehelp/blob/f4f9d900b238d03da601bf7c75cff5bcbcee6d7c/pyproject.toml#L7),
  [`#L63`](https://github.com/sphinx-doc/sphinxcontrib-applehelp/blob/f4f9d900b238d03da601bf7c75cff5bcbcee6d7c/pyproject.toml#L63)),
  so six distributions share the top-level module `sphinxcontrib`.
- The extensions import Sphinx
  ([`sphinxcontrib/applehelp/__init__.py#L14`](https://github.com/sphinx-doc/sphinxcontrib-applehelp/blob/f4f9d900b238d03da601bf7c75cff5bcbcee6d7c/sphinxcontrib/applehelp/__init__.py#L14))
  but declare it only as an extra:
  [`dependencies = []` at `#L42`](https://github.com/sphinx-doc/sphinxcontrib-applehelp/blob/f4f9d900b238d03da601bf7c75cff5bcbcee6d7c/pyproject.toml#L42),
  [`standalone = ["Sphinx>=5"]` at `#L54`](https://github.com/sphinx-doc/sphinxcontrib-applehelp/blob/f4f9d900b238d03da601bf7c75cff5bcbcee6d7c/pyproject.toml#L54).

What Meada should link (capabilities.md, "Python packages"): the six entries `exact`; an import of
`sphinxcontrib.applehelp` should be `exact` (the module matches the distribution once normalized)
while a bare `import sphinxcontrib` is `ambiguous` among six. Gaps the page implies:
`[project.optional-dependencies]` is not read, so the extensions' imports of `sphinx` have no
dependency entry to follow; extension loading by string is not an import.

Suggested fixture: two distributions `ns-a` and `ns-b` sharing a PEP 420 namespace `ns`, a
consumer that imports `ns.a` (expected `exact`) and one that imports `ns` (expected `ambiguous`),
and a provider importing its host through an extra only. Copy allowed (BSD), but model it.

### 3.5 tokio stack (Cargo)

`tokio-rs/tokio` `6470b5e4b8e943ad014383a5bf6a93ba415678a3` (890 files), `tokio-rs/mio`
`116be3af6d77584e6d0d4a9777e120f253c74d79` (89), `tokio-rs/bytes`
`c08f87663a04eac3b9ab2dfa1976852d9b525a8c` (46), `tokio-rs/axum`
`bebe175c334d2a4154b32cd9e477274c2aab5953` (506); all MIT.

- tokio's features default to none
  ([`tokio/Cargo.toml#L26`](https://github.com/tokio-rs/tokio/blob/6470b5e4b8e943ad014383a5bf6a93ba415678a3/tokio/Cargo.toml#L26)),
  and both dependencies are optional
  ([`#L100`](https://github.com/tokio-rs/tokio/blob/6470b5e4b8e943ad014383a5bf6a93ba415678a3/tokio/Cargo.toml#L100) `bytes`,
  [`#L101`](https://github.com/tokio-rs/tokio/blob/6470b5e4b8e943ad014383a5bf6a93ba415678a3/tokio/Cargo.toml#L101) `mio`).
- Use behind the `net` feature:
  [`tokio/src/net/tcp/stream.rs#L143`](https://github.com/tokio-rs/tokio/blob/6470b5e4b8e943ad014383a5bf6a93ba415678a3/tokio/src/net/tcp/stream.rs#L143)
  (`mio::net::TcpStream::connect`), provided at
  [`mio src/net/tcp/stream.rs#L91`](https://github.com/tokio-rs/mio/blob/116be3af6d77584e6d0d4a9777e120f253c74d79/src/net/tcp/stream.rs#L91)
  ([`mio Cargo.toml#L4`](https://github.com/tokio-rs/mio/blob/116be3af6d77584e6d0d4a9777e120f253c74d79/Cargo.toml#L4)).
- axum depends on bytes and on tokio through a rename to its own name
  ([`axum/Cargo.toml#L109`](https://github.com/tokio-rs/axum/blob/bebe175c334d2a4154b32cd9e477274c2aab5953/axum/Cargo.toml#L109),
  [`#L102`](https://github.com/tokio-rs/axum/blob/bebe175c334d2a4154b32cd9e477274c2aab5953/axum/Cargo.toml#L102)
  `tokio = { package = "tokio", ... optional = true }`).

What Meada should link (capabilities.md, "Rust"): the dependency entries `exact`; imports only in
items compiled with default features, so most of tokio's `mio` calls (inside `cfg_net!` and
friends) stay out by design. Worth measuring: how much of a real crate's cross-repo surface the
default-feature rule hides.

Suggested fixture: a provider crate and a consumer whose dependency is `optional = true` and used
only in a module gated by a feature outside `default`, plus one use in an ungated module; the
golden records the dependency link and the one import link, so a later decision on features
shows up as a diff. Copy allowed (MIT); model it.

### 3.6 h3 and rou3 (npm)

`h3js/h3` `f0e17808ac2439b6434bb10609a961067f4d555b` (277 files) and `h3js/rou3`
`c3bdbf5b8843905f0424395bfe444d89f8bce0b8` (90), MIT, TypeScript, ESM. (`unjs/h3` redirects here
after a transfer; a `[[peer]] repo` written with the old owner would not match a fresh clone's
remote.)

- [h3 `package.json#L57`](https://github.com/h3js/h3/blob/f0e17808ac2439b6434bb10609a961067f4d555b/package.json#L57)
  depends on `rou3`, imported at
  [`src/h3.ts#L1`](https://github.com/h3js/h3/blob/f0e17808ac2439b6434bb10609a961067f4d555b/src/h3.ts#L1)
  (`createRouter`, `addRoute`, `findRoute`).
- rou3 exports only build output,
  [`package.json#L23`](https://github.com/h3js/rou3/blob/c3bdbf5b8843905f0424395bfe444d89f8bce0b8/package.json#L23)
  (`".": "./dist/index.mjs"`), and its tsconfig emits nothing
  ([`tsconfig.json#L17`](https://github.com/h3js/rou3/blob/c3bdbf5b8843905f0424395bfe444d89f8bce0b8/tsconfig.json#L17),
  `noEmit`), so there is no `outDir` to map back; the source entry re-exports
  [`addRoute`](https://github.com/h3js/rou3/blob/c3bdbf5b8843905f0424395bfe444d89f8bce0b8/src/operations/add.ts#L54).

What Meada should link (capabilities.md, "npm packages"): the dependency `exact`; calls cross
through `exports` only when `outDir` maps the entry back to `rootDir`, which this bundler layout
(obuild, unbuild, tsup) lacks. Likely a miss today; not verified.

Suggested fixture: a provider whose `exports` point at `dist/index.mjs`, built by a bundler
config, with `src/index.ts` beside and a tsconfig with `noEmit`; the golden fixes today's answer
(dependency link only) until a rule for `dist/<name>.mjs` to `src/<name>.ts` is decided. Copy
allowed (MIT); model it.

### 3.7 Pallets (PyPI baseline)

`pallets/flask` `49943b5a94413a530761350f04e896f76e8d620e` (236 files), `pallets/werkzeug`
`2adaad1ab68312fa46efb07ff072b15b720e2444` (294), `pallets/jinja`
`5ef70112a1ff19c05324ff889dd30405b1002044` (107), `pallets/click`
`2247b35ea1c47c727d7a06e51fa280e12a863ff6` (178); BSD-3-Clause.

[Flask `pyproject.toml#L24`](https://github.com/pallets/flask/blob/49943b5a94413a530761350f04e896f76e8d620e/pyproject.toml#L24)
to [`#L29`](https://github.com/pallets/flask/blob/49943b5a94413a530761350f04e896f76e8d620e/pyproject.toml#L29)
lists `click`, `itsdangerous`, `jinja2`, `markupsafe` and `werkzeug`; providers name themselves
`Werkzeug` ([`#L2`](https://github.com/pallets/werkzeug/blob/2adaad1ab68312fa46efb07ff072b15b720e2444/pyproject.toml#L2)),
`Jinja2` ([`#L2`](https://github.com/pallets/jinja/blob/5ef70112a1ff19c05324ff889dd30405b1002044/pyproject.toml#L2)) and
`click` ([`#L2`](https://github.com/pallets/click/blob/2247b35ea1c47c727d7a06e51fa280e12a863ff6/pyproject.toml#L2)).
Imports: [`src/flask/app.py#L17`](https://github.com/pallets/flask/blob/49943b5a94413a530761350f04e896f76e8d620e/src/flask/app.py#L17)
to [`#L20`](https://github.com/pallets/flask/blob/49943b5a94413a530761350f04e896f76e8d620e/src/flask/app.py#L20),
reaching for example
[`werkzeug/exceptions.py#L16`](https://github.com/pallets/werkzeug/blob/2adaad1ab68312fa46efb07ff072b15b720e2444/src/werkzeug/exceptions.py#L16).

What Meada should link: entries `exact` after PEP 503 folding (`Jinja2` to `jinja2`), imports
`exact` (module equals the distribution). No new fixture: the value is a corpus of exact PyPI
links across repositories, which the dogfood set lacks.

### 3.8 spf13 (Go baseline, near duplicates)

`spf13/cobra` `adbc8813901bba65827259daa8e22ff94ec1f30e` (66 files, Apache-2.0),
`spf13/pflag` `f8a990678a6d44385313e9b19e40737f889f9b14` (91, BSD-3-Clause, itself a GitHub
fork of `ogier/pflag` under a new module path), `spf13/viper`
`528f7416c4b56a4948673984b190bf8713f0c3c4` (65, MIT).

[cobra `go.mod#L8`](https://github.com/spf13/cobra/blob/adbc8813901bba65827259daa8e22ff94ec1f30e/go.mod#L8)
requires pflag, imported under an alias at
[`command.go#L30`](https://github.com/spf13/cobra/blob/adbc8813901bba65827259daa8e22ff94ec1f30e/command.go#L30)
(`flag "github.com/spf13/pflag"`) and called at
[`#L1690`](https://github.com/spf13/cobra/blob/adbc8813901bba65827259daa8e22ff94ec1f30e/command.go#L1690)
(`flag.NewFlagSet`), defined at
[`pflag flag.go#L1398`](https://github.com/spf13/pflag/blob/f8a990678a6d44385313e9b19e40737f889f9b14/flag.go#L1398).
[viper `go.mod#L10`](https://github.com/spf13/viper/blob/528f7416c4b56a4948673984b190bf8713f0c3c4/go.mod#L10)
to [`#L12`](https://github.com/spf13/viper/blob/528f7416c4b56a4948673984b190bf8713f0c3c4/go.mod#L12)
require afero, cast and pflag. viper also says in comments that two functions are "mostly copied
from pflag" ([`viper.go#L1384`](https://github.com/spf13/viper/blob/528f7416c4b56a4948673984b190bf8713f0c3c4/viper.go#L1384),
[`#L1408`](https://github.com/spf13/viper/blob/528f7416c4b56a4948673984b190bf8713f0c3c4/viper.go#L1408)):
hand-labelled near duplicates across repositories for #517's normalised variants.

What Meada should link (capabilities.md, "Go modules"): `require` `exact`, aliased import `exact`
with `; import module path`. No new fixture; keep as corpus.

### 3.9 Apache Commons (Maven baseline)

`apache/commons-lang` `61837eb4472dac790a69f5f34b7b89ddfaf43db0` (719 files) and
`apache/commons-text` `aeab8401bbdb1a98d458c10f4ef6d77a86e5b4ba` (277), Apache-2.0.
[commons-text `pom.xml#L86`](https://github.com/apache/commons-text/blob/aeab8401bbdb1a98d458c10f4ef6d77a86e5b4ba/pom.xml#L86)
to [`#L88`](https://github.com/apache/commons-text/blob/aeab8401bbdb1a98d458c10f4ef6d77a86e5b4ba/pom.xml#L88)
depends on `org.apache.commons:commons-lang3` with a property version, provided at
[commons-lang `pom.xml#L28`](https://github.com/apache/commons-lang/blob/61837eb4472dac790a69f5f34b7b89ddfaf43db0/pom.xml#L28)
(group inherited from the external `commons-parent`). The call
[`StringSubstitutor.java#L1313`](https://github.com/apache/commons-text/blob/aeab8401bbdb1a98d458c10f4ef6d77a86e5b4ba/src/main/java/org/apache/commons/text/StringSubstitutor.java#L1313)
(`Validate.isTrue(cond, "msg")`, import at
[`#L27`](https://github.com/apache/commons-text/blob/aeab8401bbdb1a98d458c10f4ef6d77a86e5b4ba/src/main/java/org/apache/commons/text/StringSubstitutor.java#L27))
fits only the varargs overload
[`Validate.java#L587`](https://github.com/apache/commons-lang/blob/61837eb4472dac790a69f5f34b7b89ddfaf43db0/src/main/java/org/apache/commons/lang3/Validate.java#L587)
by argument count.

What Meada should link (capabilities.md, "Maven and Gradle packages"): dependency `exact` (parent
group inherited, computed version kept), the call `exact` with `; import package`. Corpus value;
copy allowed.

### 3.10 Serilog (NuGet baseline)

`serilog/serilog` `43105b35095337d77f022561a8f2d3db2f9a7937` (285 files) and
`serilog/serilog-sinks-console` `9b75d510787f5d54cf76584a3c3341d7eab0ee0a` (76), Apache-2.0
(`serilog-sinks-file` at `4c8e6772881a8d9c8f1b3142a753ea37e8ad2342` has the same shape).
[`Serilog.Sinks.Console.csproj#L28`](https://github.com/serilog/serilog-sinks-console/blob/9b75d510787f5d54cf76584a3c3341d7eab0ee0a/src/Serilog.Sinks.Console/Serilog.Sinks.Console.csproj#L28)
references `Serilog`, whose id is the project file name
([`src/Serilog/Serilog.csproj`](https://github.com/serilog/serilog/blob/43105b35095337d77f022561a8f2d3db2f9a7937/src/Serilog/Serilog.csproj),
no `PackageId`). The sink is an extension method
([`ConsoleLoggerConfigurationExtensions.cs#L56`](https://github.com/serilog/serilog-sinks-console/blob/9b75d510787f5d54cf76584a3c3341d7eab0ee0a/src/Serilog.Sinks.Console/ConsoleLoggerConfigurationExtensions.cs#L56))
calling `Sink` on a parameter of a provider type
([`#L77`](https://github.com/serilog/serilog-sinks-console/blob/9b75d510787f5d54cf76584a3c3341d7eab0ee0a/src/Serilog.Sinks.Console/ConsoleLoggerConfigurationExtensions.cs#L77)),
which has three overloads
([`LoggerSinkConfiguration.cs#L45`](https://github.com/serilog/serilog/blob/43105b35095337d77f022561a8f2d3db2f9a7937/src/Serilog/Configuration/LoggerSinkConfiguration.cs#L45),
[`#L61`](https://github.com/serilog/serilog/blob/43105b35095337d77f022561a8f2d3db2f9a7937/src/Serilog/Configuration/LoggerSinkConfiguration.cs#L61),
[`#L124`](https://github.com/serilog/serilog/blob/43105b35095337d77f022561a8f2d3db2f9a7937/src/Serilog/Configuration/LoggerSinkConfiguration.cs#L124)).

What Meada should link (capabilities.md, "NuGet packages"): `PackageReference` `exact`; the call on
a typed parameter by name and argument count, `ambiguous` if two overloads take three arguments.
Users calling `.WriteTo.Console()` go through an extension method, which the page says is not
followed: a documented gap with a real corpus. Copy allowed.

### 3.11 SwiftNIO (SwiftPM baseline)

`apple/swift-nio` `108daca35da6a1b4ed8da29c1dfbd19668b9b8fc` (767 files),
`swift-server/async-http-client` `d2dfb39bcb0fafd1834ec6e6334c1755360f856e` (169),
`apple/swift-log` `d0f784af9ea2bc75e557c3d2bb628a68248605f3` (144); Apache-2.0.
[`Package.swift#L48`](https://github.com/swift-server/async-http-client/blob/d2dfb39bcb0fafd1834ec6e6334c1755360f856e/Package.swift#L48)
and [`#L53`](https://github.com/swift-server/async-http-client/blob/d2dfb39bcb0fafd1834ec6e6334c1755360f856e/Package.swift#L53)
name swift-nio and swift-log by URL; the providers declare
[`swift-nio` `#L44`](https://github.com/apple/swift-nio/blob/108daca35da6a1b4ed8da29c1dfbd19668b9b8fc/Package.swift#L44)
with products `NIOCore` ([`#L46`](https://github.com/apple/swift-nio/blob/108daca35da6a1b4ed8da29c1dfbd19668b9b8fc/Package.swift#L46))
and `NIOPosix` ([`#L49`](https://github.com/apple/swift-nio/blob/108daca35da6a1b4ed8da29c1dfbd19668b9b8fc/Package.swift#L49)),
and [`swift-log` `#L6`](https://github.com/apple/swift-log/blob/d0f784af9ea2bc75e557c3d2bb628a68248605f3/Package.swift#L6)
with `Logging` ([`#L8`](https://github.com/apple/swift-log/blob/d0f784af9ea2bc75e557c3d2bb628a68248605f3/Package.swift#L8)).
Uses: [`HTTPClient.swift#L17`](https://github.com/swift-server/async-http-client/blob/d2dfb39bcb0fafd1834ec6e6334c1755360f856e/Sources/AsyncHTTPClient/HTTPClient.swift#L17)
to [`#L22`](https://github.com/swift-server/async-http-client/blob/d2dfb39bcb0fafd1834ec6e6334c1755360f856e/Sources/AsyncHTTPClient/HTTPClient.swift#L22)
(imports), [`#L98`](https://github.com/swift-server/async-http-client/blob/d2dfb39bcb0fafd1834ec6e6334c1755360f856e/Sources/AsyncHTTPClient/HTTPClient.swift#L98)
(`Logger(label:factory:)`), [`#L1255`](https://github.com/swift-server/async-http-client/blob/d2dfb39bcb0fafd1834ec6e6334c1755360f856e/Sources/AsyncHTTPClient/HTTPClient.swift#L1255)
(`MultiThreadedEventLoopGroup.singleton`).

What Meada should link (capabilities.md, "Swift packages"): `.package(url:)` `exact` by remote
identity; `Logger(label:factory:)` `exact` by labels; a static property is no call. Corpus value;
copy allowed.

### 3.12 Petclinic (HTTP, Java and TypeScript, OpenAPI)

| Repository | SHA | Files | License |
| --- | --- | --- | --- |
| `spring-petclinic/spring-petclinic-rest` | `e1987d2b4715b30f8b97050b3a399fdcadf92cb3` | 153 | Apache-2.0 |
| `spring-petclinic/spring-petclinic-reactjs` | `4a6c26d99f48d8bfac97352130b9fae4feab2509` | 207 | Apache-2.0 |
| `spring-petclinic/spring-petclinic-angular` | `328115dc1b11422fc9a3543a4755d7e2385ffd3c` | 346 | none found: measure only, do not copy |

- Provider: controllers implement interfaces that openapi-generator writes at build time, so no
  route annotation is in the repository
  ([`OwnerRestControllerV1.java#L53`](https://github.com/spring-petclinic/spring-petclinic-rest/blob/e1987d2b4715b30f8b97050b3a399fdcadf92cb3/src/main/java/org/springframework/samples/petclinic/rest/controller/v1/OwnerRestControllerV1.java#L53),
  `@RequestMapping("/api") ... implements OwnersApi`); the routes live in
  [`openapi.yml#L65`](https://github.com/spring-petclinic/spring-petclinic-rest/blob/e1987d2b4715b30f8b97050b3a399fdcadf92cb3/src/main/resources/openapi.yml#L65)
  and following, under the server
  [`#L10`](https://github.com/spring-petclinic/spring-petclinic-rest/blob/e1987d2b4715b30f8b97050b3a399fdcadf92cb3/src/main/resources/openapi.yml#L10)
  (`http://localhost:9966/petclinic/api`), matching
  [`application.properties#L24`](https://github.com/spring-petclinic/spring-petclinic-rest/blob/e1987d2b4715b30f8b97050b3a399fdcadf92cb3/src/main/resources/application.properties#L24)
  and [`#L25`](https://github.com/spring-petclinic/spring-petclinic-rest/blob/e1987d2b4715b30f8b97050b3a399fdcadf92cb3/src/main/resources/application.properties#L25)
  (port and `context-path`).
- Angular client: base from a configuration object,
  [`environment.ts#L26`](https://github.com/spring-petclinic/spring-petclinic-angular/blob/328115dc1b11422fc9a3543a4755d7e2385ffd3c/src/environments/environment.ts#L26)
  (`REST_API_URL: 'http://localhost:9966/petclinic/api/'`), used through Angular's `HttpClient`
  ([`owner.service.ts#L36`](https://github.com/spring-petclinic/spring-petclinic-angular/blob/328115dc1b11422fc9a3543a4755d7e2385ffd3c/src/app/owners/owner.service.ts#L36),
  [`#L48`](https://github.com/spring-petclinic/spring-petclinic-angular/blob/328115dc1b11422fc9a3543a4755d7e2385ffd3c/src/app/owners/owner.service.ts#L48)).
- React client: a URL builder, then `fetch`:
  [`util/index.tsx#L4`](https://github.com/spring-petclinic/spring-petclinic-reactjs/blob/4a6c26d99f48d8bfac97352130b9fae4feab2509/client/src/util/index.tsx#L4)
  (base, a global define or `http://localhost:9966/petclinic`),
  [`#L6`](https://github.com/spring-petclinic/spring-petclinic-reactjs/blob/4a6c26d99f48d8bfac97352130b9fae4feab2509/client/src/util/index.tsx#L6)
  (``url = (path) => `${BACKEND_URL}/${path}` ``),
  [`VetsPage.tsx#L20`](https://github.com/spring-petclinic/spring-petclinic-reactjs/blob/4a6c26d99f48d8bfac97352130b9fae4feab2509/client/src/components/vets/VetsPage.tsx#L20)
  and [`#L22`](https://github.com/spring-petclinic/spring-petclinic-reactjs/blob/4a6c26d99f48d8bfac97352130b9fae4feab2509/client/src/components/vets/VetsPage.tsx#L22).
  This repository also carries its own copy of the backend, with the same Maven coordinates as
  the rest repository
  ([reactjs `pom.xml#L7`](https://github.com/spring-petclinic/spring-petclinic-reactjs/blob/4a6c26d99f48d8bfac97352130b9fae4feab2509/pom.xml#L7),
  [rest `pom.xml#L7`](https://github.com/spring-petclinic/spring-petclinic-rest/blob/e1987d2b4715b30f8b97050b3a399fdcadf92cb3/pom.xml#L7),
  versions 3.2.1 and 4.0.2): a second real case of two repositories serving one package name,
  with no consumer.

What Meada should link (capabilities.md, "HTTP and contract documents"): OpenAPI routes as
providers of the repository with `[serve]`, each with a gateway form under the document's server
path (`src/http.rs`, `Http::declare`); `fetch` consumers whose URL folds; no Angular consumer
(not a listed client). Results in section 4.3.

Suggested fixture: `http-url-builder`, a TypeScript client with
`` const url = (p: string) => `${BASE}/${p}` ``, `fetch(url("api/vets"))` and a `const u = url(..)`
local passed to `fetch`, with `BASE` a literal and, in a second file, `process.env.API_URL`;
the provider serves `/api/vets`. Copy allowed (Apache-2.0) from the rest and reactjs
repositories; nothing from the Angular one.

### 3.13 RealWorld (HTTP, one OpenAPI document, many implementations)

`gothinkster/realworld` `c5aecdf9a2e0cf68e8420a7cae55db8d06d45f87` (334 files, MIT),
`sveltejs/realworld` `df796708040f5200ec572b28ab7f88ecee5794dd` (62, MIT),
`gothinkster/golang-gin-realworld-example-app` `626c372d259472148d93303f74aa9b9a1cdcef24`
(39, MIT); dozens of other backends and frontends implement the same API.

- The document: [`specs/api/openapi.yml#L20`](https://github.com/gothinkster/realworld/blob/c5aecdf9a2e0cf68e8420a7cae55db8d06d45f87/specs/api/openapi.yml#L20)
  (`https://api.realworld.show/api`), [`#L22`](https://github.com/gothinkster/realworld/blob/c5aecdf9a2e0cf68e8420a7cae55db8d06d45f87/specs/api/openapi.yml#L22)
  (`/users/login`).
- The Svelte client: literal base
  [`src/lib/api.js#L3`](https://github.com/sveltejs/realworld/blob/df796708040f5200ec572b28ab7f88ecee5794dd/src/lib/api.js#L3),
  a `fetch` wrapper [`#L17`](https://github.com/sveltejs/realworld/blob/df796708040f5200ec572b28ab7f88ecee5794dd/src/lib/api.js#L17),
  called through SvelteKit's `$lib` alias
  ([`src/routes/login/+page.server.js#L2`](https://github.com/sveltejs/realworld/blob/df796708040f5200ec572b28ab7f88ecee5794dd/src/routes/login/+page.server.js#L2),
  [`#L14`](https://github.com/sveltejs/realworld/blob/df796708040f5200ec572b28ab7f88ecee5794dd/src/routes/login/+page.server.js#L14)),
  an alias defined only in the generated `.svelte-kit/tsconfig.json`; most pages are `.svelte`
  files, which Meada does not read.
- The Go server: gin groups passed into registration functions
  ([`hello.go#L41`](https://github.com/gothinkster/golang-gin-realworld-example-app/blob/626c372d259472148d93303f74aa9b9a1cdcef24/hello.go#L41),
  [`#L42`](https://github.com/gothinkster/golang-gin-realworld-example-app/blob/626c372d259472148d93303f74aa9b9a1cdcef24/hello.go#L42);
  [`users/routers.go#L10`](https://github.com/gothinkster/golang-gin-realworld-example-app/blob/626c372d259472148d93303f74aa9b9a1cdcef24/users/routers.go#L10)
  to [`#L13`](https://github.com/gothinkster/golang-gin-realworld-example-app/blob/626c372d259472148d93303f74aa9b9a1cdcef24/users/routers.go#L13)).

What Meada should link: the document's routes with the `/api` server path as a gateway form; the
client's wrapper calls by host (`[serve] hosts = ["api.realworld.show"]` on the server); the gin
routes only if a group passed as a function argument keeps its prefix (capabilities.md states
that for Ktor, Vapor and C# minimal APIs, not for gin). Several servers implementing one document
also make each client call fit several providers, an HTTP analogue of #392.

Suggested fixture: a gin server whose `/api` group is passed to `UsersRegister(g *gin.RouterGroup)`
in another package, and a JavaScript client importing its wrapper through a tsconfig `paths`
alias. Copy allowed (MIT).

### 3.14 OpenTelemetry demo (Kafka across languages)

`open-telemetry/opentelemetry-demo` `33bd823107b1b38eaa88dd4ef625cc16d60475d4`, 624 files,
Apache-2.0; each service is a directory, so a fixture would make each one a repository.

- Producer (Go, sarama `AsyncProducer`): topic from environment with a literal default,
  [`src/checkout/kafka/producer.go#L18`](https://github.com/open-telemetry/opentelemetry-demo/blob/33bd823107b1b38eaa88dd4ef625cc16d60475d4/src/checkout/kafka/producer.go#L18)
  to [`#L23`](https://github.com/open-telemetry/opentelemetry-demo/blob/33bd823107b1b38eaa88dd4ef625cc16d60475d4/src/checkout/kafka/producer.go#L23)
  (`KAFKA_TOPIC`, else `"orders"`), the send at
  [`src/checkout/main.go#L671`](https://github.com/open-telemetry/opentelemetry-demo/blob/33bd823107b1b38eaa88dd4ef625cc16d60475d4/src/checkout/main.go#L671)
  to [`#L683`](https://github.com/open-telemetry/opentelemetry-demo/blob/33bd823107b1b38eaa88dd4ef625cc16d60475d4/src/checkout/main.go#L683)
  (`ProducerMessage{Topic: kafka.Topic}` sent on `Input()`), broker from `KAFKA_ADDR`
  ([`#L248`](https://github.com/open-telemetry/opentelemetry-demo/blob/33bd823107b1b38eaa88dd4ef625cc16d60475d4/src/checkout/main.go#L248)).
- Consumer (C#, Confluent.Kafka):
  [`src/accounting/Consumer.cs#L31`](https://github.com/open-telemetry/opentelemetry-demo/blob/33bd823107b1b38eaa88dd4ef625cc16d60475d4/src/accounting/Consumer.cs#L31)
  (`GetEnvironmentVariable("KAFKA_TOPIC") ?? "orders"`),
  [`#L46`](https://github.com/open-telemetry/opentelemetry-demo/blob/33bd823107b1b38eaa88dd4ef625cc16d60475d4/src/accounting/Consumer.cs#L46)
  (`Subscribe`).
- Consumer (Kotlin, plain `kafka-clients`, no Spring):
  [`main.kt#L26`](https://github.com/open-telemetry/opentelemetry-demo/blob/33bd823107b1b38eaa88dd4ef625cc16d60475d4/src/fraud-detection/src/main/kotlin/frauddetection/main.kt#L26),
  [`#L57`](https://github.com/open-telemetry/opentelemetry-demo/blob/33bd823107b1b38eaa88dd4ef625cc16d60475d4/src/fraud-detection/src/main/kotlin/frauddetection/main.kt#L57).

What Meada should link (capabilities.md, "Messaging channels"): in C#, an environment read leaves
the side unresolved, so `?? "orders"` gives no channel; Kotlin and Java Kafka count only through
Spring Kafka, so the Kotlin consumer is outside the table; the Go side depends on whether a send
on sarama's `Input()` channel counts as a producer. Expected today: few or no
`channel::kafka::orders` links. HTTP base URLs have `base_url_env`; channel names have no
counterpart, which this set argues for.

Suggested fixture: `channels-env-default`, a Go producer and a C# consumer whose topic is an
environment read with a literal fallback, recording today's unresolved sides so a decision on
defaults shows up as a diff. Copy allowed (Apache-2.0).

### 3.15 go-yaml copies (#517, Go)

`go-yaml/yaml` tag `v3.0.1`, `f6f7691b1fdeb513f56608cd2c32c51f8194bf51` (archived repository;
MIT for the code ported from libyaml, Apache-2.0 for the rest) and `kubernetes-sigs/yaml` tag
`v1.4.0`, `c3772b51db126345efe2dfe4ff8dac83b8141684` (67 files, MIT and BSD-3-Clause; a GitHub
fork of `ghodss/yaml`). The second holds a fork of the first under `goyaml.v3/`
([`goyaml.v3/README.md#L9`](https://github.com/kubernetes-sigs/yaml/blob/c3772b51db126345efe2dfe4ff8dac83b8141684/goyaml.v3/README.md#L9),
"based on v3.0.1"). Comparing blob SHAs: 12 Go files are byte-identical (`apic.go`, `decode.go`,
`encode.go`, `parserc.go`, `readerc.go`, `resolve.go`, `scannerc.go`, `sorter.go`,
`suite_test.go`, `writerc.go`, `yaml.go`, `yamlprivateh.go`) and 7 Go files differ (`emitterc.go`,
`yamlh.go` and five tests). Later releases of `kubernetes-sigs/yaml` replace the copy with a
dependency on `go.yaml.in/yaml`, so the tag matters.

Use: a corpus with known ground truth for `search kind=duplicate` (every symbol of the 12 files
should group across the two repositories) and for #517's variants (the changed functions of
`emitterc.go` are near copies). Copy allowed (MIT, Apache-2.0, BSD-3-Clause), but measure, do
not vendor.

### 3.16 packaging copies (#517, Python, three-way)

`pypa/packaging` tag `26.3`, `929fd4b1410ac7ef61ef3f45b2f5d7e87711a9b5` (Apache-2.0 OR
BSD-2-Clause); `pypa/pip` `ec43c48b2f2de7bfaac32bee85ee33a9e496db07` (1,104 files, MIT), which
pins it at
[`src/pip/_vendor/vendor.txt#L5`](https://github.com/pypa/pip/blob/ec43c48b2f2de7bfaac32bee85ee33a9e496db07/src/pip/_vendor/vendor.txt#L5);
`pypa/setuptools` `049ae8e3083514378aedb352a2017fbc306bd126` (564 files, MIT), which vendors it
under `setuptools/_vendor/packaging/`. Of the 22 `.py` files, setuptools' copy matches all 22
blobs and pip's matches 16 (`markers.py`, `ranges.py`, `requirements.py`, `specifiers.py`,
`tags.py` and `version.py` are patched by pip's vendoring).

Use: three-way exact groups plus patched near copies, in the language where #517's renaming
variants are least studied here. Note for #392: neither vendored copy has a `pyproject.toml`, so
neither becomes a provider of `packaging`.

## 4. Meada verification

Meada 0.1.0 as installed on 2026-10-27, exports from the consumer clone, links counted with `jq`
by contract prefix and level. That binary predates #421, #422, #424, #425 and #468 on main
(Python source roots and Rust item macros among them), so the counts below may differ on a
current build. The rules the findings rest on (a `replace` by another module stays out, PyPI
ranges are unchecked, `fetch` is the only TypeScript client, a document's server path gives the
gateway form) read the same in capabilities.md and `src/http.rs` on main; the runs were not
repeated.

### 4.1 gogo fork

Workspace: `tendermint/cns` with peers `gogo/protobuf` and `regen-network/protobuf`; `meada
status` reports 39, 460 and 465 indexed files. The export took 110 s at about 0.9 GB peak RSS,
mostly the two copies of generated protobuf code.

| Run | Consumers in cns | Links |
| --- | --- | --- |
| As published (with `replace`) | 0 | 0 |
| `replace` line deleted locally | 23 (the `require` and 22 calls such as `proto.CompactTextString`) | 46, all `package::go` `ambiguous`, reason `2 providers fit; ... v1.3.3 (local none), range unchecked` |

Misses: with the `replace`, the dependency on `github.com/gogo/protobuf` disappears, together
with every call into it, although the replacement's repository is a peer. capabilities.md does
say "a replacement by another module stays out", so this is the documented rule, but the result
is a consumer with no package link where the code names its provider exactly. Without the
`replace`, the result is the #392 trigger as the issue describes it: one entry, two providers,
`ambiguous` with no way to decide. A report to the maintainer, not filed: whether a `replace`
whose target module is a workspace repository should link to it (`exact`, the target's repository
being named), which would make the as-published case decided by the code itself.

### 4.2 uv, pubgrub fork and uv's own workspaces

Workspace: `astral-sh/uv` at `0.9.0` with peers `astral-sh/pubgrub` and `pubgrub-rs/pubgrub`
(531, 18 and 18 indexed files); export 6.7 s, 0.2 GB. 585 links:

| Contract prefix | Level | Provider repository | Links |
| --- | --- | --- | --- |
| `package::cargo` | `exact` | `astral-sh/uv` (its own crates) | 494 |
| `package::cargo` | `exact` | `astral-sh/pubgrub` | 61 (6 dependency entries, 55 `use` sites with `; import crate name`) |
| `package::cargo` | any | `pubgrub-rs/pubgrub` | 0 |
| `package::pypi` | `ambiguous` | `astral-sh/uv` | 26 (`bird-feeder` 16, `seeds` 6, `black` 4) |
| `package::pypi` | `exact` | `astral-sh/uv` | 4 (`anyio` 3, `first-local` 1) |

The Cargo side is right: the `git` URL picks the fork, and the upstream with the same crate names
gets nothing. The PyPI side shows three things, all from uv's own test data:

- `bird-feeder` and `seeds`: every entry fits all same-named packages (four and three), although
  `[tool.uv.sources] ... = { workspace = true }` and `[tool.uv.workspace] members` name exactly
  one. Expected under today's rules; the decidable case of #392, and decidable from the manifest
  if Meada read `[tool.uv.sources]`.
- `black`: `docs/requirements.txt` pins PyPI's `black==24.4.2`, and Meada links it, `ambiguous`,
  to `ecosystem/black` (a manifest copy, `local none`) and to `scripts/packages/black_editable`
  (`local 0.1.0`, which the `==` pin rejects). Both candidates are wrong: the real provider is
  outside the workspace. Likely false positive; checking a PEP 440 `==` pin against the local
  version would drop one of them.
- `anyio`: three consumers that take `anyio` from PyPI link `exact` to a lone in-repo stub named
  `anyio` (version `4.3.0+foo`, which satisfies `>=4,<5`). Likely false positive with a confident
  level: a single same-named package in the workspace is not evidence that it is the provider.
  For the maintainer, not filed: whether a PyPI or npm link to a package that only test data
  declares should be capped below `exact`, or whether `[tool.uv.sources]` (and npm's
  `workspace:` protocol) should be what makes it `exact`.

### 4.3 Petclinic

Workspace: `spring-petclinic-rest` with `[serve] hosts = ["localhost:9966"]` and its OpenAPI
document listed; the React and Angular repositories as consumers with the rest repository as
peer. `meada status` reads the document (`openapi 3.0.1, rotas: 37`). Each export took under
3 s.

| Repository | HTTP providers | HTTP consumers | Links |
| --- | --- | --- | --- |
| rest | 37 from the document, `http::ANY::/` from code | | |
| reactjs | `http::ANY::/` from its own backend copy | 14: 13 unresolved, 1 `GET /api/oups` (a literal `http://localhost:8080/...`) | 0 |
| angular | | 7, all from an unrelated `docs/template-playground-app/app.js` | 0 |

Misses:

- The React client's 12 `fetch` calls whose URL comes from the `url(path)` builder are
  unresolved: a call to a function that returns a template string is not folded, while a wrapper
  that calls `fetch` itself is (`http-wrappers`). Fixture candidate (section 6).
- The Angular client's 25 `this.http` calls outside tests are invisible: Angular's
  `HttpClient` is not a listed consumer. Documented gap.
- Code routes: none from either backend, since both implement generated `*Api` interfaces;
  capabilities.md says annotations inherited from an interface are not read, and here the
  interface is not even in the repository. The document is the only source, which the `[serve]
  documents` path handles.
- Not a bug: the document's server path `/petclinic/api` is applied as a gateway form
  (`Http::declare` in `src/http.rs`), and `[serve] prefix` is then not used for document routes.
- Real drift the clients carry: the React client calls `/api/owner/...` and `/api/oups`, the rest
  document serves `/owners/{ownerId}` and `/oops`; the React client was written against its own
  older backend.

## 5. What was not verified

- Sets 4 to 11 and 13 to 16 were read through the API only; their expected links come from
  capabilities.md, not from a run.
- No public pair of separate repositories with a broker producer and consumer, or with AsyncAPI
  documents describing real code, was found; set 14 is a monorepo.
- No real vendored copy that keeps its manifest (a checked-in Cargo crate, `package.json` or
  `go.mod` under `vendor/` or `third_party/`) was found; `go mod vendor` drops `go.mod` files, and
  the Python copies of set 16 keep only `dist-info`. uv's `ecosystem/black` is a manifest copy
  without code.
- The license of `spring-petclinic-angular` (no LICENSE file found) and the clause-level text of
  the sphinxcontrib `LICENCE.rst` files were not confirmed.

## 6. Fixtures to write first, and corpora to keep

Fixtures, in order:

1. `gopkg-replace` (from 3.1): `orig` and `fork` repositories that both declare
   `module example.com/orig/lib`, and a consumer with `require example.com/orig/lib v1.3.3` and
   `replace example.com/orig/lib => example.com/fork/lib v1.3.3-fork.1`, importing and calling
   the package. Today: no consumer. It pins the rule #392's ADR would revisit, and a variant
   without the `replace` gives the `ambiguous` trigger itself.
2. `pkg-uv-sources` (from 3.2): one repository with two `bird-feeder` packages under two
   workspace roots, consumers choosing by `[tool.uv.sources]` (`workspace = true`, `path`), a
   `requirements.txt` pin `bird-feeder==24.4.2` that neither local version satisfies, and a lone
   stub shadowing a registry name. It records today's `ambiguous` and `exact` answers so the
   false positives of 4.2 show up as diffs when the rule changes.
3. `http-url-builder` (from 3.12): TypeScript `` const url = (p) => `${BASE}/${p}` ``,
   `fetch(url("api/vets"))` and `const u = url("api/owners"); fetch(u)`, with `BASE` literal and
   from `process.env`, against a provider of `/api/vets` and `/api/owners`. Today: unresolved.
4. `cargo-optional-feature` (from 3.5): a dependency with `optional = true` used in a module
   gated by a non-default feature and once outside it, so the default-feature rule's effect on
   package links is fixed in a golden.
5. `pkg-dist-exports` (from 3.6): an npm provider whose `exports` point at `dist/index.mjs` with
   a `noEmit` tsconfig and `src/index.ts` beside it, and a consumer calling an exported function;
   records whether the call crosses.

Next after these: `channels-env-default` (3.14) and a PEP 420 namespace pair (3.4).

Corpora worth keeping, at the pinned SHAs above:

- For #392: the gogo fork set as published and with the `replace` removed (the trigger in its
  plainest form, at real size); `astral-sh/uv` at `0.9.0` alone (four `bird-feeder`, three
  `seeds`, a `black` manifest copy, an `anyio` stub); the pubgrub pair as the control where a
  `git` URL already decides; the two petclinic backends serving one Maven coordinate.
- For #517: `go-yaml/yaml@v3.0.1` with `kubernetes-sigs/yaml@v1.4.0` (12 identical Go files, 7
  changed); `pypa/packaging@26.3` with pip and setuptools (22 and 16 identical files, 6 patched);
  `spf13/viper` with `spf13/pflag` for hand-labelled near copies. Each gives ground truth by blob
  SHA, which the dogfood set cannot, and complements the dogfood measurement #517's brief asks
  for.
