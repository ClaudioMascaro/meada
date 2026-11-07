# Resource declarations

`resource.rs`'s
  declarations (spec #19) read no code; `declare` turns each `[[resource]]` `via` symbol, IaC address or indexed config key the repository has into a
  provider (writer) or consumer (reader) of `resource::<kind>::<id>`, and `link` joins every
  declared reader to every declared writer of another repository at `declared`, reason the contract. The
  [AWS SDK use sides](../contracts.md#aws-sdk-use-sides-659-660) add code extraction and exact local SDK links. A `via` that
  names no location is dropped. An entry without `via`, or with none matched, makes the repository
  the side: the `site` is the bare repository identity (no `::`), the **repository-level** side.
  `report` gives the status lines per repository: each unmatched `via`, the fallback to repository
  level, and a resource with no link in the workspace (unlinked).

## Declared resource locations (#641)

`ContractKind::read_declarations` loads indexed locations before `declare` in both full and
partial queries. `resource/via.rs` resolves only this repository's declaration keys through
`config_values::sites`; nothing is read from disk, including in server mode. The query retains
canonical key identities and the selector spelling in `Export::config_sites`; scalar values
never enter the export. `resolved` takes exact symbols first, then the supported IaC address
forms and key locations. IaC addresses are bare, file-qualified or full identities; root-query
qualifiers are not declaration forms; a bare address matching several source identities is
stale and needs a file qualifier. Several evaluations of one source block share its source
identity. The existing status messages and repository fallback are retained.

Config key identities are `<repository>::<file>#<canonical key path>`, distinct from the
repository-level location of a directionless inferred use. Resource context lists directed
sides and their linked counterparts; generic query/trace placement carries the key and line.
The shared export includes an IaC node holding a provider as well as a consumer.
