# Cloud modules

Opt-in modules, off by default and documented below. A module is a cargo
feature plus one section of `meada.workspace.toml`; the release binaries are the default build,
with none, and a section in a build without its feature is an error naming the feature. The
`cloud` feature turns them all on at once (`cargo install meada --features cloud`) and, like
`refresh-lock`, builds on Unix only.

Choose modules for your deployment. Each is opt-in: both the binary feature and its manifest section are required.

- [Snapshots](snapshots.md)
- [Generations and retention](generations.md)
- [Refresh and publication](refresh.md)
- [Authentication and grants](admission.md)
- [GitHub App credentials](github-app.md)
- [Management console](console.md)
- [Enterprise identity: Postgres, audit, GitHub grant and directory](enterprise.md)
