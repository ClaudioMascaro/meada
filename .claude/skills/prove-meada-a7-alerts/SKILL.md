---
name: prove-meada-a7-alerts
description: "Meada proof flows for alert contracts and incident routing (A7). Read after /prove-meada."
disable-model-invocation: true
---

# Prove alerts and incident routing (A7)

Read `.claude/skills/prove-meada/SKILL.md` first: it owns Scope, Sort, the answer-key rules, the verdicts and the receipt, and it sets `S`, `RUN` and `BASE`. Every A7 flow also follows the **Copies** and **Could fool you** paragraphs of [prove-meada-a7-config](../prove-meada-a7-config/SKILL.md).

## Answer keys

| Flow or surface | Flow kind | Answer key | Rung | Where it lives | Written by |
|---|---|---|---|---|---|
| A7 Datadog monitor tags, OpsGenie criteria and explicit Alertmanager receiver transforms to incident fields | new | #673/#642/#638, provider-native condition semantics and explicit Alertmanager notifier mapping; existing alert answers use the base binary | 4 + 1 | `incident-routing.py`, [references/incident-routing.md](../prove-meada/references/incident-routing.md) | maintainer spec and provider documentation; fresh checker required |

## Flows

| Flow | Stories | Surfaces | Entries (`$S/infra.tsv`, `$S/corpus.tsv`) | Check | State |
|---|---|---|---|---|---|
| Alert labels and Alertmanager routes | #672; 53, 54, 57 to 60 | CLI export/status, MCP stdio/HTTP, console graph | neutral generated monitoring/routing/app repositories; pinned `kube-prometheus` | `alerts.py --head-bin BIN --run DIR --http`, then the graph and public-source checks of [alerts.md](../prove-meada/references/alerts.md) | landed (#672) |
| Datadog, OpsGenie and PagerDuty conditions with explicit receiver forwarding | #673; 53 to 60 | CLI export/status, MCP stdio/HTTP, console graph | original #638 plus neutral Datadog and Alertmanager receiver configuration | `incident-routing.py --head-bin BIN --run DIR --http`, then independent controls in [incident-routing.md](../prove-meada/references/incident-routing.md) | #673 |
