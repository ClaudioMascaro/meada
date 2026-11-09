# Azure SDK clients (#670)

The five groups share `contract/sdk/origins.rs` and its existing `EVAL_DEPTH` and `EVAL_BUDGET`;
no Azure-specific recursive walker or new evaluation ceiling exists. Memo keys retain complete
structured argument identity and exact construction frames. Hidden/import-ambiguous clients,
typed-only origins, reassigned handles and opaque return branches are absent. Mutable argument
containers use the shared scoped effect guard for both identity folding and memo normalization.

The ADR 0026 inventory is explicit. Event Hubs needs a separately written hub; connections and
credentials are never parsed. Blob keeps a known container with an unknown object. Unsupported
operations, direct URL client constructors and other async/polling families yield no form.
Known clients with unread names yield unresolved sides. Pure Azure identifiers use the bounded
helpers listed in [Terraform and provisioning](resolution-the-index-and-shape-readers/terraform-and-provisioning.md).

Receiver JSON stores only subtype and normalized literal/env/unknown name components. Workload
companions join by indexed workload/root/container scope, avoiding a workspace-wide Cartesian
product; conflicting or unread same-scope values prevent certainty. Legacy None is unchanged.
The schema golden version invalidates an older layout. C# legacy Service Bus forms retain their
original topic identities and ceilings; new polling receivers use qualified subscriptions.
