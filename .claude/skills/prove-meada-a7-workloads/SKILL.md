---
name: prove-meada-a7-workloads
description: "Meada proof flows for workload environments, AWS workloads, images and anchored names (A7). Read after /prove-meada."
disable-model-invocation: true
---

# Prove workloads and images (A7)

Read `.claude/skills/prove-meada/SKILL.md` first: it owns Scope, Sort, the answer-key rules, the verdicts and the receipt, and it sets `S`, `RUN` and `BASE`. Every A7 flow also follows the **Copies** and **Could fool you** paragraphs of [prove-meada-a7-config](../prove-meada-a7-config/SKILL.md).

## Answer keys

| Flow or surface | Flow kind | Answer key | Rung | Where it lives | Written by |
|---|---|---|---|---|---|
| A7 AWS function/task images, direct code, environment overrides and path refusal | new | #664/#642 and ADR 0025/0026, independently generated input inventory; HTTP copies stdio | 4 + 1 | `aws-workloads.py`, [references/aws-workloads.md](../prove-meada/references/aws-workloads.md) | maintainer spec; independent checker |

## Flows

| Flow | Stories | Surfaces | Entries (`$S/infra.tsv`, `$S/corpus.tsv`) | Check | State |
|---|---|---|---|---|---|
| Workloads and image contracts: normalized builders/runners, direct Compose code paths, evaluated names/patterns, scope-aware orphans | #662; 44 to 46, 57 to 60, 63, 64 | CLI export/status, MCP stdio/HTTP, console API and graph | neutral independently generated repositories and pinned `boutique` | `images.py --public --http` with the UI options of [images.md](../prove-meada/references/images.md) | landed (#662) |
| Anchored unresolved channel/resource/image names and decisions | #674; #642 29, 30, 58 | CLI export/status/decide, MCP stdio/HTTP, console graph | generated neutral repositories | `patterns.py --head-bin BIN --run DIR --http`, plus `images.py` graph ([patterns.md](../prove-meada/references/patterns.md)) | landed (#674) |
| Workload environment entries | #663; 47 to 49 | CLI export, MCP stdio and HTTP | generated sibling workload/code/queue repositories | `environment.py --head-bin BIN --run DIR` ([environment.md](../prove-meada/references/environment.md)) | #663 |

For #663 workload environment resolution, run `python3 $S/environment.py --head-bin "$RUN/bin/meada-head" --run "$RUN/environment"` and inspect the saved source, resolved side/provision sets and raw transport answers ([references/environment.md](../prove-meada/references/environment.md)).

For #664 AWS functions/tasks, run `python3 "$S/aws-workloads.py" --head-bin "$RUN/bin/meada-head" --run "$RUN/aws-workloads"`. Inspect whole producer/image/execution sets, direct and image traces, environment provenance, orphan scope, path-only refresh and refusal captures. [references/aws-workloads.md](../prove-meada/references/aws-workloads.md) names the independent inputs, controls and limits; the script supplies evidence for an independent verdict.

For #662's images, run `python3 $S/images.py --base-bin "$RUN/bin/meada-base" --head-bin "$RUN/bin/meada-head" --run "$RUN/images" --http --public --ui-bin "$RUN/bin/meada-head-ui" --ui-dir ui/dist`. Judge the complete captures against #662/#642 and the base ADRs, including pinned boutique's source, the full export deltas, evaluated/pattern levels, both trace directions, visibility and real graph views. Retain every optional field for same-mode export parity. Run the reference's `images-mixed.py` (when the baseline supports AWS) and `image-ko.py` supplements too: preserve combined storage/trace refresh and refuse ko shell/help false publications. Shared contract/query changes also require all-corpus base parity and console API; launch/version changes require the viewer page budgets.

For #674 anchored names, run `python3 $S/patterns.py --head-bin "$RUN/bin/meada-head" --run "$RUN/patterns" --http`. Read [references/patterns.md](../prove-meada/references/patterns.md), review every candidate and level, and retain CLI decision/stale/restore captures. Existing image and console graph flows still run through `images.py`.
