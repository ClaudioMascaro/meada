# Changelog

All notable changes to Meada are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project follows
[Semantic Versioning](https://semver.org/).

## [0.1.0] - Unreleased

The first release: the trunk of the roadmap (ADR 0003). The maintainer sets the date when
tagging.

### Added

- Indexing of TypeScript and JavaScript repositories (`.ts`, `.tsx`, `.js`, `.jsx`, `.mjs`,
  `.cjs`) with tree-sitter: definitions, imports (relative paths, `index` resolution and
  `tsconfig.json` `paths` aliases) and calls, each with a level (`exact`, `inferred` or
  `ambiguous`).
- A call through a local binding (`let`, `var`, `catch`, destructuring, `for...of`, nested
  parameter patterns) never resolves `exact` to the definition, constant or parameter of the
  same name that it hides.
- HTTP contracts linked across repositories: `fetch` consumers (including client wrappers and
  base URLs read from environment variables) matched to Next.js app router and Express
  providers. Every link carries a level and a reason; calls whose URL cannot be resolved are
  reported as unresolved, never linked.
- `meada.toml` with `[[peer]]` (`repo`, `path`, `base_url_env`) and `[serve]` (`hosts`). Errors
  name the file, the line and the key.
- Workspace discovery through the normalized `origin` git remote, with peers and sibling clones
  found locally. Peers that are not cloned are reported and the rest still answers.
- A per-repository SQLite index in `.meada/`, which ignores itself in git, refreshed on every
  query from files changed on disk, including uncommitted and untracked ones.
- The `meada` command with `index`, `status`, `export` (sorted, byte-reproducible JSON) and
  `mcp`. Messages follow `LANG`: Portuguese or English.
- An MCP server over stdio with three read-only tools, `search`, `context` and `trace`, whose
  answers are compact English lines under a token budget, with a chain footer on `trace`.
- Packages as contracts (spec #22): each named `package.json` provides `package::npm::<name>`,
  and each dependency entry that names a package of the workspace (any repository, its own
  included; a `workspace:` range keeps its own) links to it, flagged
  `range mismatch` or `range unchecked` when the range does not plainly accept the provider's
  version. A call to a function imported from such a package links to the function in the
  provider, across repositories, through `exports`, `types`, `module`, `main` and re-exports,
  and through the `tsconfig.json` `outDir` back to the source. `search` takes the kind
  `package` (repository, directory, version and consumers), and `context` on an exported
  function lists its consumers, from any repository of the workspace, with the range flag of
  each.
- Community files in Portuguese, English and Spanish: README, CONTRIBUTING, code of conduct,
  security policy and issue and PR templates; a CI workflow running `cargo fmt --check`,
  `cargo clippy` and `cargo test`.
- A release workflow built with cargo-dist: from a version tag it builds binaries for Linux,
  macOS and Windows on x86_64 and arm64 and a shell installer, published to GitHub Releases.
  The crate is ready for `cargo install meada`.

### Measured

The maintainer ran this release on the private dogfood set on 2026-10-14 (ADR 0003).

Links. One consumer repository calls the provider from four sites in its current code (ADR 0003
counted five); all four link to the right route at `inferred`, through `base_url_env`. The other
consumer repository's three calls do not link: its client receives `fetch` as a parameter and
takes the URL from a destructured object, two patterns this release does not read (#157, #158).

Cost, on the provider repository at the 2026-10-11 commit nearest to 633 files (634 TypeScript
and JavaScript files), median of three runs on a 16-thread Linux laptop:

| Metric | Target | Result |
| --- | --- | --- |
| Cold index time | ≤ 7 s | 1.2 s |
| Cold index size | ≤ 37 MB | 21.8 MB |
| Single-file reindex | ≤ 1 s | 0.24 s |
| Tool list | ≤ 1k tokens | 454 tokens |
| Default answer | ≤ 2k tokens | 1,834 tokens, the largest of 11 answers |

At today's 664 files: 1.3 s, 23.6 MB and 0.26 s. Tokens count 4 bytes each, as the answer budget
does. These numbers gate 1.0, not this release.

[0.1.0]: https://github.com/ClaudioMascaro/meada/releases/tag/v0.1.0
