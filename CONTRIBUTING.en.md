# Contributing to Meada

[Português (Brasil)](CONTRIBUTING.md) · **English** · [Español](CONTRIBUTING.es.md)

> This is a translation. The Brazilian Portuguese [guide](CONTRIBUTING.md) is the source; if
> they disagree, it wins, and a fix to this page is welcome.

Thank you for wanting to help. This guide says how to set up your environment, how to propose a
change and what the project expects of you. By taking part, you agree to follow the
[Code of Conduct](CODE_OF_CONDUCT.en.md).

## Which language to write in

- **Issues, PRs and reviews** may be written in Portuguese, Spanish or English. We reply in the
  language you used.
- **Code, identifiers, comments, commit messages and ADRs** are in English, so that anyone can
  read them.
- **Documentation for people** (the README, this guide, the code of conduct) is born in
  Brazilian Portuguese. The `.en.md` and `.es.md` files are translations, in that order of
  priority. If you change a Portuguese file, update both translations in the same PR; if you
  cannot, open an issue labelled `translation` for each one left behind.
- Did a passage in the English or Spanish read oddly? Fixing a translation is a great first
  contribution.

## Before you start

- Look for an open issue that already covers your idea. If there is none, open one. For large
  changes, talk it over in the issue before writing code.
- For a wrong Meada conclusion (drift, link or level), use the `inference` form, next to the bug
  and feature ones; replace private names with placeholders such as `repoA` and `fn1`.
- Issues labelled `good first issue` are meant for newcomers, and `help wanted` marks where help
  is needed most.
- The v1 scope and the order of work are in
  [ADR 0003](docs/adr/0003-v1-scope-and-roadmap.md).
- Use the terms in [`GLOSSARY.md`](GLOSSARY.md) in code, docs and issues: *repository*, *index*,
  *peer*, *provider*, *consumer*, *contract*, *link*, *level*, *chain* and the rest.
- The project's decisions live in [`docs/adr`](docs/adr). A change that goes against one of them
  starts with a new ADR. In particular, the product invariants (no LLM or network in indexing,
  local-first, no write tools in MCP, one index per repository, determinism) are in
  [ADR 0002](docs/adr/0002-product-invariants.md).

## Clean-room rule

Meada covers ground that GitNexus (PolyForm Noncommercial license) also covers. To keep the
project clean:

- build from Meada's own ADRs and specs;
- do not copy or read GitNexus source code, its npm package's `dist/` or its tuned constants,
  and do not paste any of that into issues, PRs or commits.

## Set up your environment

You need Rust. The `rust-toolchain.toml` file pins the compiler version and installs `rustfmt`
and `clippy` by itself when you use [rustup](https://rustup.rs).

```sh
git clone https://github.com/ClaudioMascaro/meada.git
cd meada
cargo build
cargo test
```

## Before you open the PR

These three commands are the ones CI runs on the default build (`fmt`, `clippy`, `test`), and
all three must pass:

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```

A change under a cloud module's feature `<f>` (ADR 0012) also runs
`cargo clippy --all-targets --features <f> -- -D warnings` and `cargo test --features <f>`. CI
also has the `cloud` job: it checks that the default build carries no module crate and runs
clippy and the tests with `--all-features`, against a Valkey and an S3-compatible server (the
tests that need them read `MEADA_TEST_VALKEY_URL` and `MEADA_TEST_S3_ENDPOINT`, and without
them say so and skip).

If the change touches behavior, include the test that covers it.

## Site documentation

Pages live in `docs/site/pt`, `docs/site/en` and `docs/site/es`, with the same filename in all
three languages. The [build guide](docs/engineering/documentation-build.md) explains dependency
installation, local preview and publication. Before submitting a change, run
`mkdocs build --strict` and `python scripts/docs/check.py` in the documentation environment.
These commands check links, anchors, translations and diagrams, in addition to the Rust gates above.

## Commits and PRs

- **Conventional Commits**: `feat:`, `fix:`, `docs:`, `refactor:`, `test:`, `chore:`, `ci:`,
  with an optional scope. Example: `fix(resolve): keep the link level when a route has a prefix`.
- Name the branch after the type: `feat/...`, `fix/...`, `docs/...`.
- Open the PR against `main` and fill in the template. Link the PR to the issue
  (`Closes #123`).
- The PR is merged by **squash merge**: the branch becomes a single commit on `main`, and the PR
  title becomes that commit's message. So the PR title follows Conventional Commits too.
- The `fmt`, `clippy` and `test` checks must be green before merging.
- Keep the PR small and single-purpose. Unrelated changes go in separate PRs.

## Security

Do not open a public issue for a vulnerability. See the [security policy](SECURITY.en.md).
