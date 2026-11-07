# Resource declarations

`resource.rs` (spec
  #19): reads no code; `declare` turns each `[[resource]]` `via` symbol the repository has into a
  provider (writer) or consumer (reader) of `resource::<kind>::<id>`, and `link` joins every
  reader to every writer of another repository at `declared`, reason the contract. A `via` that
  names no symbol is dropped. An entry without `via`, or with none matched, makes the repository
  the side: the `site` is the bare repository identity (no `::`), the **repository-level** side.
  `report` gives the status lines per repository: each unmatched `via`, the fallback to repository
  level, and a resource with no link in the workspace (unlinked).
