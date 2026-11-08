# HTTP through infrastructure (#665)

`http/infrastructure.rs` prepares HTTP links once in `Http::workspace_ready`, after all kinds
finish their workspace steps (including image pattern decisions). Its transient `Export::http`
holds links and structured keys for the inferences a symbol's context explains.
`Http::workspace_links` returns them; the ordinary per-call `Http::link` remains the fallback. The `explain_inferred` hook receives a link and workspace
exports; HTTP shares the prepared decision with rendering, without parsing a human reason.
Declared `base_url_env` peer explanations retain their previous policy.

`iac::Node::ingress` stores literal host/prefix rules and their transformed Service addresses.
An empty Service address is an unread mount, including rewrites, unsupported path types and
unresolved backends. `kube/routing.rs` reads that evidence; `kube::Eval::unit` applies the same
namespace and evaluation root as other references, charging strings before copying them.
The existing stored `iac_references` supply selector targets. Index/Engine transport these
facts and make no HTTP decisions. All routing is over visible exports, with no source or
network read at query time.

Kubernetes DNS forms accept a short name, a namespace suffix, `.svc` and `.svc.cluster.local`,
each with an optional numeric port. A known caller workload confines short names to its
namespace, deployment repository and root; explicit namespaces may cross namespaces inside
that root. Without a known caller all matching visible candidates remain candidates.
Compose names stay in their caller's unit and shared network. A literal code/config URL and
a workload-scoped environment URL share this routing. `Environments::scoped_values` reuses
the existing literal lookup, adding workload/root identities and unread entries while
preserving the `values` API and the common scope rules.

Selectors reach the image kind's shared effective execution projection, honoring valid runner
choices and refusing remaining ambiguous image matches; direct executions confine provider sites to their safe local code path. Ingresses match a
literal host and the longest segment prefix; an unread longer rule blocks shorter rules.
Their prefix is also an internal provider gateway form, so `serves`, candidates, decision
replay and drift agree. Alias expansion is charged before copying; an exhausted table makes
Ingress mounts unread as a whole (see the
[Kubernetes ceilings](../../ceilings/resolution-the-index-and-shape-readers/kubernetes-and-crossplane.md)).
Several distinct providers remain ambiguous. HTTP links through Service/Compose/Ingress or a
workload environment are inferred and retain their evidence. A route that reaches no code (no
selected workload, an unread mount, an exhausted alias table) prepares no link, so the per-call
`Http::link` still links a host a repository declares in `[serve] hosts`.

Tests: `tests/it/http_iac.rs`, sibling `http-iac-*` fixtures and DNS units in the HTTP module.
`http-iac.py` in `/prove-meada` drives neutral sources through CLI and MCP stdio/HTTP,
including deletion of every source file supplying new routing facts. Read-only queries and
answer-sized/every-row equivalence use the stored facts alone.
