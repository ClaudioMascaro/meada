# Image contracts (#662)

`image.rs` owns builders, runners, matching and workload execution; `image/config.rs` reads
GitHub Actions tags/images, Skaffold artifacts, literal Maven/Gradle Jib destinations and ko
commands with checked-in `KO_DOCKER_REPO` and `--bare`. A ko base image is an input, never a
builder. Only one simple literal ko command is read; multiline and compound shell scripts
stay absent. The recognized command has one `--bare`, at most one literal target and no other
flags; an explicit shell is `bash` or `sh`. No process environment is read. Configuration
facts enter the stamped `contract_configs` table through `ContractKind::config_file` and
`config_summary`; `config_reads_paths` rechecks Compose paths when only the tree changed.

The kind declares repository-level providers with `at`, workload consumers from stored Compose
and Kubernetes images, and Helm `image.repository` consumers. `iac/types.toml` marks workload
types. Image normalization expands Docker Hub names, removes tags/digests and lowercases names.
Unknown interpolations remain patterns; the IaC pattern matcher enforces the existing literal
and segment ceilings. A literal name is exact, defaults and Helm values inferred (a Helm `image.registry` prefixes
`image.repository`), and distinct matching image candidates ambiguous, including two images
built by the same repository. Duplicate build evidence for one repository and image is collapsed
to its strongest evidence, and a repository's literal name covers its own pattern matching the
same runner.

`ContractKind::executions` lets the shared trace follow workloads into repository entry points
and code back to workloads. Same-repository direct paths are exact without an image link (inferred when the path went
through a value a deployer can override, such as `${CTX:-./app}`) and
appear in the optional `executions` export array; a direct execution replaces a workload's
image side in its own root only (`image::runs_locally`). Paths are normalized inside the root and
cross no symlink; a changed path is rechecked even when the Compose file did not change.
`scope_status` and `side_note` implement the two-role orphan rule in the visible workspace; an
unresolved image name (`image::*`, a `docker/metadata-action` output) is neither role nor orphan.
`exports_repository_links` includes repository-level builders in their own export without
changing the export scope of earlier kinds. IaC spans also let ambiguous workload consumers
be open items with the provider's own image identities as decision candidates.

Image consumer/link `root` fields retain evaluation scope. `meada-core::export::side_id` is the
single rule for a workload image's decision identity (site, contract and root); older kinds
retain the call-site identity. Item grouping and decision replay use that identity, while
context and trace keep the workload's site. Helm's configuration-derived nodes participate in
the same visible-export lookup as stored IaC nodes, so their exported identities start queries.
Mixed consumer declaration passes and their sort order retain the evaluation root: identical
workload sites and normalized images in distinct overlays stay distinct runners and links.
Repeated image evidence for one workload and root still collapses to one side.

## Workload environment resolution (#663)

`Image::workspace_step` reuses image links and safe direct executions to resolve directed
channel/resource environment reads. The workspace hook receives repositories as well as exports
so builder selection has the same evidence as ordinary image linking. Container images, workload
identities, evaluation roots and direct build paths scope every match. The stored environment
stays out of the graph export; workload context lists its names and targets. A resolved side's
`via` carries `workload:<identity> (root <root>), env:<name>`; the contract evidence seam keeps
provisioning and ordinary directed links inferred and names that provenance in their reasons.
`channel/message.rs` retains an optional environment name, including the legacy TypeScript SQS
forms, separately from an unresolved channel identifier. These legacy sides carry a pending
workload marker in storage, cleared before query output when no workload resolves the read;
unmatched sides retain their earlier absent `via`. Established SDK `env:` provenance stays intact.

`image::execution_links` projects raw image links through the existing generic decision replay
for workload scope. Environment and HTTP derivation share it; raw links remain the input for
items, validation and stale/waiting reports, so a choice cannot invalidate itself.
`image::environment::Environments::new(exports, repositories)` collects those scoped entries;
`values(repository, site, variable)` returns only values for that code site, with their workload
and variable explanations. HTTP base resolution can reuse this lookup without duplicating
builder matching. Several workloads resolving one side to one contract contribute their reasons
to a single side. Workspaces with no environment entries avoid the extra image-link pass.
Construction indexes executions by workload/root and builders by workload/root/image, reusing
each image match for its container entries. Queries index repository, variable and build path,
visiting only the source file and its ancestor paths.

## Image pattern choices (#674)

`image/patterns.rs` owns choices for an unresolved builder name that matches several literal
runner names (#674). Their `image-builder::<repository>::<pattern contract>` identity belongs
to the builder repository. Duplicate runner sites of one literal name remain one name;
choosing it retains every runner of that name. `ContractKind::consumer_item` distinguishes
these source items from a runner's ordinary ambiguity among builders. `owns_decision`,
`decision_items` and `decision_reports` keep source-item rules within the kind while the engine
combines their transport with ordinary decisions. Pattern matching remains shared with IaC;
NATS and RabbitMQ routing patterns retain their existing broker semantics.

`Image::workspace_step` prepares distinct runner candidates once for each builder pattern in
query-only `Export::name_candidates`, including a valid selection's decision position.
Linking, open items, replay and orphan checks reuse that cache. It is rebuilt per query, so
changed or hidden runners cannot persist as candidates. `patterns.rs` exercises 2,000 runner
names through `Engine::open_items` under a time bound.

Item hydration, listing and ordinary replay share the seam's `consumer_item` eligibility.
Candidate reconstruction and replay share `link_candidate`, which checks the builder's active
name, so an inactive pattern cannot return through another contract at the same repository
site. A runner choice becomes `item-not-open` when its remaining ambiguity belongs to the
builder; an inactive target of a still-open runner item becomes `not-candidate`.
