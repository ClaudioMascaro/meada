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
- Community files in Portuguese, English and Spanish: README, CONTRIBUTING, code of conduct,
  security policy and issue and PR templates; a CI workflow running `cargo fmt --check`,
  `cargo clippy` and `cargo test`.
- A release workflow built with cargo-dist: from a version tag it builds binaries for Linux,
  macOS and Windows on x86_64 and arm64 and a shell installer, published to GitHub Releases.
  The crate is ready for `cargo install meada`.

[0.1.0]: https://github.com/ClaudioMascaro/meada/releases/tag/v0.1.0
