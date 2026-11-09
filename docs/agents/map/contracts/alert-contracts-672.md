# Alert contracts (#672)

`alert.rs` owns `alert::<key>::<value>`, with emitter providers and matcher consumers.
`alert/config.rs` reads monitoring configuration through the shared YAML/JSON event reader,
then `declare` creates rule and route IaC nodes from the stored summaries. Queries never read
the checkout. Equality conditions are exact, anchored regular expressions inferred; negated
conditions only filter, with no consumer row of their own. Matching checks the entire rule's
labels against the route's conjunction, including ancestor conditions; unknown conditions
cannot create links. `context_at` supplies the receiver. The kind follows the scope-aware
two-role orphan rule of ADR 0026.

Matcher-free root fallbacks have no alert side: no label key/value is written. Routing
conditions, inherited receivers and sibling `continue` are read, while the graph does not
predict final notifications, inhibition or time intervals. Unknown label values do not select
a later non-continuing sibling by guess. Literal Grafana `grafana_rule_group.rule` labels
also come from HCL/OpenTofu and Terraform JSON; dynamic blocks, expressions and a group
repeated with `count` or `for_each` (made any number of times, zero included) are not emitters.
A file with a `values` word in its name (`values.yaml`, `prod-values.yaml`, `values.prod.yaml`)
or under `templates/` is Helm input, not monitoring configuration. A quoted matcher reads
Alertmanager's escapes (`\n`, `\"`, `\\`); any other backslash stays in the value. An
AlertmanagerConfig route also matches its own namespace, which is not read: its conditions are
`inferred`, their consumers' `via` ending in `(AlertmanagerConfig namespace)`.

`ContractKind::workspace_links` optionally replaces the per-consumer hook for a kind;
alert uses it to compile each route tree once and evaluate each rule once per tree. Optional
`workspace_related_sides` and `workspace_status` group query results in repository order;
their default `None` retains existing per-repository hooks and order. Alert context prepares
links before hydrating sites, then reuses them to find and render related sides. Alert status
reuses existing links and runs at most one extra routing pass when an empty result needs a
limit diagnostic. Unrelated repositories do not restart the matching budget or repeat routing.
Identical regexes share compilation, and distinct ones share the automaton budget. Missing
expression-result labels are unknown.
Prometheus/Loki add `alertname` after their label maps, so it is a literal emitter even without
an explicit `labels` map. A computed HCL or Terraform JSON label key makes that label map opaque.

`alert/regexp.rs` validates raw ASCII capture names before translating Go/RE2 ASCII classes,
quoting, octal escapes and class literals. It checks counted repetitions and canonical Unicode
names, then uses the bounded Rust engine.
Route explanations are shared until a link is emitted. The link pass charges each retained
field before allocation; on overflow status reports the limit instead of orphan results.
The reader also charges the derived sides of ancestor conditions and every field of projected
rule/route IaC nodes to its file budget. Pending children hold their parent index and borrow
inherited receivers only when visited, so a wide route list cannot copy a long receiver before
the budget stops it. Site placement asks only the contract's kind. Route visits and condition
checks consume a workspace evaluation budget even when nothing matches.

## Incident routing (#673)

`alert/incident.rs` reads literal Datadog tags, OpsGenie criteria and PagerDuty PCL
conditions from Terraform/OpenTofu, including JSON. Native monitor and OpsGenie resource
identities reuse their IaC nodes. Only `key:value` tags declare emitters; plain tags remain
payload facts because another condition can test them. Nested orchestration rules get their own source-located
nodes. Rule labels, OpsGenie tags and PagerDuty event fields are separate input streams.
`alert/forwarding.rs` turns only a selected Alertmanager receiver's explicit configuration
into incident fields. No query reads credentials or infers a team from an equal label.

OpsGenie receiver tags use literal text and simple label substitutions fixed by the route's
inherited grouping. PagerDuty v2 receiver details become the corresponding native
`event.custom_details` field, as Alertmanager's notifier explicitly writes it. Other templates,
missing label values and absent forwardings remain unknown. The receiver's written team
restrictions narrow matching when supplied. Matching describes compatible checked-in rules,
not actual delivery, final escalation, inhibition or time restrictions.

The shared tri-state condition evaluator handles conjunction and disjunction, regex, prefix
and containment. An unread condition cannot make a conjunction true or let a possibly consumed
event fall through to a later non-continuing rule. Native rule ordering is applied within its
written scope. Unexpanded OpsGenie instances and missing criteria remain internal ordering
barriers, without projected base sites. Dynamic criteria remain unknown; an unread team
scope cannot prove a destination in another team group of the same module. Stored and
workspace budgets charge forwarding expansion as well as emitted
links. The original public #638 rule tests `service:app`; the trace from a monitor with both
service and support tags preserves that distinction.
