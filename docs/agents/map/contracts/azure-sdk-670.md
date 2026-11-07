# Azure SDK use sides (#670)

`contract/azure/sdk.rs` selects five language policies, carries `Handle` stages and folded
identity components through the shared `contract/sdk/origins::Evidence` walk, and emits sides.
The `ts`, `python`, `java`, `go` and `csharp` children own verified origins, factory transitions,
argument positions and operation inventories. `channel` and `resource` invoke the reader for
their own roles. Resource reads retain operation call sites; channel polling receivers retain
the enclosing handler. No client/handle-only call emits a side.

`contract/azure/sdk/environment.rs` owns subtype validation and same-scope companion joins.
`contract/receiver.rs` defines neutral private `ReceiverIdentity` (subtype + normalized name
parts) and scoped environment values. `Found.identity`, internal Provider/Consumer summaries
and the optional `contracts.identity` JSON carry it, without changing the public export schema.
The row key includes identity; the existing schema-golden hash rebuilds older source indexes.
`image/environment::receiver_values` supplies certain stored values keyed by workload, root
and container. `ContractKind::environment_identity` is optional; legacy None keeps the prior
normalization hooks. The collector does not interpret cloud names.

Pure normalization remains `contract/azure.rs`: Service Bus slash queues differ from
`subscription:<topic>/<sub>`, Blob uses container identity, and groups/partitions/objects/secret
versions never identify contracts. Constructor credentials and connection strings are discarded.
Existing C# Service Bus send/processor policy remains in `channel/csharp.rs` unchanged.
ADR 0026 and the user guide contain the bounded operation inventory. Public Engine fixtures
cover all five groups, both JS/TS source languages, exact sibling factories, hidden/reassigned/
opaque/name-matched controls, structured identifiers, scoped/typed environments and source-deleted
stored queries. The permanent `azure-sdk.py` proof flow adds CLI/MCP transport and refresh checks.

Internal side sorting retains the optional identity; declaration deduplication preserves it
while a contract is unresolved. A queue and
subscription may share a handler, unresolved prefix and environment variable; dropping either
before workload resolution would erase a real qualified side. Legacy None retains prior keys.

Receiver overload policy folds only the second argument through the guarded shared API.
TS object form and unshadowed explicit undefined prove queue options; C# accepts the verified
options constructor, whose category also remains distinct in normalized memo arguments.
Opaque/reassigned actuals cannot reuse a preceding options factory result. String arguments
continue to identify subscriptions; legacy C# send/processor paths are unchanged.
