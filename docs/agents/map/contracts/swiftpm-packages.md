# SwiftPM packages

- SwiftPM packages (spec #29, ticket #124): `package/swiftpm.rs` implements `Ecosystem` for
  `Package.swift`, read by `package/swiftpm/manifest.rs` (`package`, on the grammar readers of
  `lang/swift/manifest.rs`, beside `targets`). It provides
  `package::swiftpm::<name>` from the literal `Package(name:)`; `.library` products and their
  targets go to `Summary::products` and `targets`. A `.package(url:)` is a `git <identity>` entry
  (ADR 0004), a `.package(path:)` a `path <dir>/Package.swift` one (`sourced`), both `exact` with
  the written requirement as range; an interpolated URL or path is a `computed` entry, matched
  by its literal parts, which makes the link `ambiguous`. An `import Module` of an exposed target
  reaches the public declarations of its files by call shape and argument labels
  (`How::Module`, `; import module`), `ambiguous` when several packages expose the module.
  `Package.resolved` is not read. The stored `manifests` summary gains `products` and `targets`.
