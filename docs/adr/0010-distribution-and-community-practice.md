# 10. Distribution and community practice

- Status: accepted
- Date: 2026-10-14

## Context

Meada ships as a single static binary (ADR 0001), and its first users are agents configured
through MCP. The project also wants contributors from Brazil and Latin America, which needs
the community files to exist from the first release. This ADR fixes how Meada is installed and
what the repository has in place for contributors.

## Decision

### Distribution

- From v0.1: `cargo install meada`, prebuilt binaries on GitHub Releases built with cargo-dist
  (Linux, macOS and Windows, on x86_64 and arm64), and a shell installer.
- An npm wrapper (`npx meada`) ships before 1.0, because agent MCP configurations often launch
  servers with `npx`.
- The crate is published at v0.1, the first real release. crates.io discourages placeholder
  crates, and a publish cannot be undone, only yanked. The npm name is published with the
  wrapper.

### Community practice

- Code of conduct: Contributor Covenant 3.0. If no official pt-BR translation exists, Meada
  translates it and sends the translation upstream. A short pt-BR preamble ties it to the
  principles in the README.
- At v0.1 the repository also has CONTRIBUTING (pt-BR first, then English and Spanish),
  SECURITY.md, issue and PR templates in the three languages, and the labels `translation`,
  `good first issue` and `help wanted`.
- A CI workflow runs `cargo fmt --check`, `cargo clippy` and `cargo test`. Its checks become
  required on the `main` branch ruleset.

## Considered options

- **Covenant 2.1 in pt-BR.** Superseded by 3.0.
- **An original code of conduct.** Fair enforcement rules are hard to write.
- **A placeholder crate at project start.** crates.io discourages it, and the name could not be
  taken back.
