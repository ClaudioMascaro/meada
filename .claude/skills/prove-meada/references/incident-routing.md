# Incident routing: #673

This extends the permanent application skill through `/create-prove-it`. The primary surface
is MCP, with CLI export/status and HTTP-to-stdio parity. The existing console graph consumes
these alert rows; its graph/query checks remain in the alert flow. This adds no new UI surface
or UI performance budget. The user's minimal local validation policy leaves the full corpus,
feature matrix and integration to GitHub CI. The parent coordinates independent final proof.

Run from the repository root with a binary copied while holding the shared Cargo lock:

```bash
python3 .claude/skills/prove-meada/scripts/incident-routing.py \
  --head-bin "$RUN/bin/meada-head" --base-bin "$RUN/bin/meada-base" \
  --run "$RUN/incidents" --http
```

The script preserves inputs and SHA256s, CLI stdout/stderr/exit status, queries, MCP answers,
refresh/removal/restoration evidence and process cleanup under the run directory. It reports
checks passed, never a proof verdict. New answers use #673, #642, original #638 and the
provider sources below. Existing alert answers use the base binary on identical input;
HTTP answers additionally copy stdio. No acceptance answer key is this script's own output.

A fresh checker reads those answer keys, drives the flow and inspects complete exported
provider, consumer and link sets. In #638 the unchanged routing condition tests `service:app`.
Its monitor also has `support:team-b`; trace from the shared monitor site reaches the service
matcher. Original Helm values and nonstandard monitor YAML remain configuration only. Do not
add a support condition to that rule or claim it demonstrates final team notification.

Independently probe these branches after the scripted drive: equality, regex, prefix and
containment; mixed conjunctions and disjunctions; rule order; a missing tag, an unknown tag,
an unsupported field/operator/regex/template; no receiver, no explicit tags, and a receiver
filtered out by its route; a transformed tag whose key differs from its label; grouped labels;
a scoped receiver; multiple repositories, `.meadaignore` and hidden/filtered peers; removal
and restoration; Terraform/OpenTofu and JSON spans/shadowing. PagerDuty tags receive no implicit
forwarding. PagerDuty v2 `details` must be explicitly configured and its PCL field must name
the configured detail. Do not map arbitrary labels to `custom_details` by equality.

Sources to read, never source code from unrelated products:

- [Issue 673](https://github.com/ClaudioMascaro/meada/issues/673), parent #642 and public #638.
- [OpsGenie routing schema](https://github.com/opsgenie/terraform-provider-opsgenie/blob/master/website/docs/r/team_routing_rule.html.markdown).
- [PagerDuty service orchestration](https://github.com/PagerDuty/terraform-provider-pagerduty/blob/master/website/docs/r/event_orchestration_service.html.markdown)
  and [router](https://github.com/PagerDuty/terraform-provider-pagerduty/blob/master/website/docs/r/event_orchestration_router.html.markdown).
- Alertmanager's [OpsGenie notifier](https://github.com/prometheus/alertmanager/blob/main/notify/opsgenie/opsgenie.go)
  templates `tags` then splits on commas; its [PagerDuty notifier](https://github.com/prometheus/alertmanager/blob/main/notify/pagerduty/pagerduty.go)
  templates the configured `details` and writes v2 `payload.custom_details`.

Record the exact commit, binary hash, spec sources, scope, checker independence, every command
and any failed step in the receipt. A driver pass leaves independent source/graph/CI work
unproved until actually run. Preserve evidence during cleanup; terminate only servers this
run started and verify they exited.
