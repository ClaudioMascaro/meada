# HTTP routes declared in IaC (#666)

`iac/http.rs` reads a finite API Gateway inventory after ordinary Terraform evaluation and
workload projection. Native and JSON blocks retain the same arguments. V2 routes join exact
API/integration identities; quick create needs an explicit method/path. `iac/http/rest.rs`
joins REST API/resource/public method, walking the literal resource parent chain. Integration
transport POST never becomes the public route method. REST requires a Lambda invocation URI;
v2 also accepts a bare function ARN. Only the URI supplies Lambda evidence;
incidental dependencies, multiple/unknown selections and incompatible APIs supply none.

`iac/attribute.rs` shares exact local/module/output aliases and instance selection with
subscription endpoints. It carries overridable selection evidence and rejects collapsed
unknown expansions. It reads identities and explicitly modeled declarations, never arbitrary
deployment-time attributes. No cloud policy is added to the engine or language readers.

SAM's existing evaluator records Api/HttpApi method/path fields. Serverless records separate
known string HTTP fields while reading its bounded event list, so a path cannot masquerade
as a method. Ref/object event labels and unknown sentinels never become literal HTTP fields.
All formats store `Node.routes`; HTTP's ordinary config-summary dispatch consumes those facts.

`http/iac.rs` normalizes literal contracts through the existing HTTP normalization. Handler
modules are checked only while indexing, by `image/code.rs::checked_handler`, through the
same safe-path seam used for executions. An archive remains opaque. An exact stored symbol
or export alias gives the provider site; otherwise the repository does. `Http::declare`
then applies ordinary hosts/documents/gateways. Distinct handlers remain distinct providers.

`ContractKind::read_declarations` runs before `symbols_read`, in both full and partial reads.
HTTP uses `index::exports_of(paths)` and its path index to hydrate only validated handler
modules' stored exact exports. Resource's existing declaration hook reads indexed config
locations and needs no hydrated symbols. Neither hook reads source or evaluates IaC.

Tests: `tests/it/iac_http_routes.rs`, `iac-http-*` fixtures and whole export/query goldens.
The bounded summary/hostile probes exercise storage and expansion ceilings. The prove flow
`iac-http-routes.py` compares complete source-derived provider/link sets, a code HTTP baseline,
MCP stdio/HTTP and read-only queries after deleting IaC and handler sources. Partial/every-row
equivalence is audited by suppressing requested handler symbols, and by omitting alias
resolution only in the partial symbol request (direct controls pass before the alias fails).
