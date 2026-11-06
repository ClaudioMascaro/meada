# Azure contracts and Functions (#669)

`contract/azure.rs` owns ARM, blob/vault URL and plain-name validation and canonical Azure
identity projection. It is reused by `iac/types.rs`, `iac/config_uses.rs`, subscription
evaluation, AsyncAPI and later Azure SDK forms. Account `azurestorage` and container
`azureblob` remain separate. Service Bus targets use `subscription:<topic>/<subscription>`;
the colon cannot collide with a legal queue/topic name. Preserve slash-bearing queue names.
The helper caps identifier text before splitting or allocating components, and rejects
wrong providers/types, missing segments, unsupported suffixes and internal-marker input.

`iac/azure.rs` projects provider names independently from physical `Node.name`, composes
subscription names through known topic references under the existing evaluator budgets,
and indexes rule targets once. A declared rule makes that route inferred. Unknown composite
targets keep an explicit unresolved projection; early budget exhaustion never enables a
physical-name fallback. Physical patterns meet the existing literal/segment limits before the
fixed subscription marker is added. `iac/subscription.rs` stores/routes the resulting source and target
with root and overridable evidence. Legacy C# topic processors keep their prior behavior.

Function workloads reuse `Node.images`, typed environment entries and safe direct execution.
Only Linux reads Docker application stacks. Both OS types read local `zip_deploy_file` and
`app_settings`; Key Vault setting references have no resolved value. The image kind validates
paths during indexing, rechecks tree changes, and stores executions for query/server mode.
No archive inspection or parent-directory scope is inferred. See ADR 0026 and the translated
capabilities/user guide for the type and typed-key inventories.

`tests/it/azure.rs` drives sibling repositories through Engine export, context, trace and
status. The fixtures include per-table rows, ARM/data URL and typed-key cases, broker and
account/container separation, filtered/same-name subscriptions, literal slash queues,
legacy C# clients, Functions images/archives and unresolved environment references.
