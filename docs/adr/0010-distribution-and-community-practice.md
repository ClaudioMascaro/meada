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
- An npm wrapper ships before 1.0, because agent MCP configurations often launch servers with
  `npx`. (Amended 2026-10-30.) The package is `@meada/meada`, under the project's npm
  organization: npm would not create the unscoped `meada`, which sits next to `mead` and `meda`,
  and the registry rejects names too similar to existing packages.
- (Amended 2026-10-22; #279.) Releases also publish the OCI image
  `ghcr.io/claudiomascaro/meada`, for Linux amd64 and arm64, as `<version>` and `latest`
  (default build), and `<version>-cloud` and `latest-cloud` (`--features cloud`). Prereleases
  get no `latest*` tag. One multi-stage Dockerfile builds from source with the pinned Rust
  toolchain and `cargo build --release --locked`; the runtime is Debian bookworm-slim with
  git (at least 2.38) and CA roots, a non-root user and its home, `ENTRYPOINT ["meada"]` and
  no default command. Manifests, mirrors and indexes are operator mounts. A separate container
  workflow publishes the images when a release is published; cargo-dist's generated workflow
  stays unchanged. The base is refreshed only with a Meada release. Signing, attestations and
  SBOM are deferred until an operator requests them.
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
