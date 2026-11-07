# GCP SDK use sides (#668)

`contract/gcp/sdk.rs` and its `ts`, `python`, `java`, `go` and `csharp` children own the
documented GCP constructor, child-handle, operation and identity argument inventory. JavaScript
shares TypeScript policy. Pub/Sub publishes name topics; receives name subscription-qualified
targets. GCS reads/writes name buckets even with unknown object keys; Secret Manager access
names the secret. Constructor and child-handle creation alone supply no use direction.
The exact supported forms and official SDK references are in ADR 0026, not in language packs.

`contract/sdk/origins.rs::Evidence` is the shared complete constructor walk. A kind policy
supplies constructor and child-handle evidence. The legacy AWS `Origins::call` boolean API
wraps that same walk and memoization. Exact repository returns retain their actual call
frames; memo keys retain kind-owned normalized argument identity and client evidence, distinguishing two
named handles from one factory without expanding equivalent return lattices. Scalar, object
and list name evidence remain distinct (including destructured parameters); shared immutable
keys avoid copying structured evidence at every memo entry. Unknown branches,
hidden bindings, reassignment and name-matched hops invalidate evidence. Existing evaluation
depth and work ceilings apply. No pending or private origin data enters exports.

`Code::value_within_at` folds a selected identity argument in those frames and at its honest
call site, retaining verified external helper names and their resource subtype until projection.
`resolve/mutations.rs` shares scoped, ordered selector effects and exact alias/caller binding
between scalar selection and complete memo arguments. Missing or exhausted write knowledge
stays unknown; a later write, another object or another field does not erase an earlier value.
A constructor without a stored call site uses conservative all-writes mode. Default objects
with writes remain unknown across invocations. `contract/sdk/mutations.rs` supplies standard
mapping-call effects, with no SDK names in packs or resolution. Both syntax evidence bytes
and query work use existing ceilings. Ordinary `Code::value` folding stays unchanged. TS/JS retain complex receiver expressions so
inline `new` keeps construction evidence while a plain class call does not. `Code::value_bound` guards static factories
against a value hiding the class name. Go records the first result of a multiple-result call
as guarded scope/name and call-range evidence, separate from scalar constants. Only
`Code::first_result` and the shared SDK walk follow it; other result bindings and ordinary
value folding remain unknown. Await syntax and
asynchronous definitions remain neutral facts; the kind and shared evidence walk require a
C# async factory result to be awaited once. Resource readers use operation call sites;
channel receives retain handler definition sites. SDK policy remains here.

`ContractKind::environment_name` and `environment_target` let each kind normalize a plain or
typed workload entry while preserving the stored SDK operation. Pub/Sub receives therefore
retain `subscriptions/` after a plain ID resolves, and a typed topic cannot become a receive
target. Resource policy uses the same GCP physical helpers as Terraform/config projections.
The common environment collector owns workload scopes and `Entry.contract`; the orchestration
layer parses no GCP or SDK strings. Without an actual matching workload, environment reads
remain unresolved and retain their public `env:NAME` provenance.

`tests/it/gcp_sdk.rs` drives Engine exports over the five groups, factory frames, refusal
controls and Cloud Run environment resolution, with complete goldens. The permanent
prove-meada GCP SDK flow drives generated source through CLI and both MCP transports;
its answer keys are #668/#642, base ADRs and the official SDK inventory, never these fixtures.
