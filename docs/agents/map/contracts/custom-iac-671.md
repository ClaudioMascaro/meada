# Curated operators and private mappings (#671)

`iac/types.toml` owns the bounded inventory, not the engine: 43 custom provisioning entries,
three Rabbit topology roles and six registry module source/input rows. Every entry has an
evaluated fixture census; `tests/it/custom_iac.rs` compares whole source-authored provisioner
inventories, including disabled/unknown creation and identity controls. Capabilities in pt/en/es
list the exact supported groups and kinds. No all-CRD claim or module download is implied.

`iac/mapping.rs` validates repository `[[iac.module]]` and `[[iac.custom_resource]]` configuration,
including bounded key paths and unknown-entry errors. An exact source wins over the longest
component-bounded prefix. `mapping/modules.rs` reuses Eval for inputs, guards, count/each and
override evidence. Native type aliases use `Entry::canonical`, `mapped` and `project` so each
cloud kind owns normalization. No unrecognized ARN/URL falls back to a raw target.

`Projection` remains an optional existing JSON column. None means native physical-name behavior;
Some with name None explicitly forbids fallback; Some with a name provides the canonical target.
Optional `entry` selects a curated type, `defining` retains identity key paths for config-use
exclusion, and `code` identifies a declared code input and whether it must be a file. Optional
`binding` retains a queue contract, exchange name and routing pattern. All new members default
when reading older JSON and are omitted when empty. Projection and workload bytes share the
existing root/repository budgets. No public export or SQLite column is added for mappings.

`kube/custom.rs` owns Kubernetes key-path reading, explicit metadata defaults, namespace-local
qualifier/source references and conservative composition patch invalidation. Node.name stays
physical even when projection is absent. Cloud SQL is instance/database; Pub/Sub subscriptions
and Service Bus subscriptions preserve their native distinct identities. Crossplane requires an
explicit external-name annotation, including literal composition bases.

`kube/topology.rs` owns Queue/Exchange/Binding roles and root-local endpoint lookup by physical
name, cluster reference kind/name, namespace and vhost. Duplicate addresses/endpoints and unread
patches establish no binding. A real unique queue/exchange pair plus a supported Exchange type
prepares the stored binding. Exchange-to-exchange references remain references only. Types are
topic, direct without wildcard keys and fanout; headers/unknown types produce no queue route.

`Channel::declare` projects stored bindings without creating exchange providers. The neutral
`ContractKind::provision_uses` hook runs once after all workspace steps; its default is empty.
Channel prepares Routes once, then projects each actual publish's reached queue, matched level
and evidence into transient `Export.provision_uses`. Provisioning consumes those uses without
rewriting raw producers. This activates only with AMQP provisioners, preserving existing
code-only exports. `Export::written` includes binding-bearing IaC nodes.

Registry Lambda/ECS use #664's existing workload/environment readers and container scopes.
`image/code.rs` checks explicit scalar source_path or opaque existing package paths at indexing
time, with provenance in Projection.code; no guessed module directory, archive execution,
unread partial environment or filesystem access in evaluator/query paths.

Relevant gates: custom_iac, native AWS workload regressions, type census, kube/Crossplane,
channel/channels/channel_queries, provisioning/config uses, stored-format defaults, docs and
the focused custom-topology hostile row. The registered proof driver is
`.claude/skills/prove-meada/scripts/custom-iac.py`; its receipt is separate from CI and independent
proof. Limits route through the Terraform, Kubernetes and channel ceiling topics.
