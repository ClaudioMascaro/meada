---
name: prove-meada-a7-config
description: "Meada proof flows for config values and IaC nodes (A7, spec #642), with what every A7 flow shares. Read after /prove-meada."
disable-model-invocation: true
---

# Prove config values and IaC nodes (A7, spec #642)

Read `.claude/skills/prove-meada/SKILL.md` first: it owns Scope, Sort, the answer-key rules, the verdicts and the receipt, and it sets `S`, `RUN` and `BASE`.

## Answer keys

| Flow or surface | Flow kind | Answer key | Rung | Where it lives | Written by |
|---|---|---|---|---|---|
| A7 config values (`search` kind `value`, the status config line) | new | spec #642's user stories 1 to 17, through `prove.py values`'s own reading of the files | 4 | `prove.py values` | the maintainer; the oracle reads the files, never Meada |
| A7 IaC nodes, Terraform (`search` kind `iac`, `context` and `trace` on an address) | new | ticket #649's criteria and Terraform's address rules, through `tf_oracle.py`'s own reading of the files (python-hcl2) | 4 | `prove.py iac` | the maintainer; the oracle reads the files, never Meada |
| A7 static evaluation of Terraform names (names, patterns, `overridable:`, `count` and `for_each` over resolved values) | new | ticket #650's criteria and ADR 0025's evaluation rules, through `tf_names.py`'s own evaluation of python-hcl2's expression text | 4 | `prove.py iac` | the maintainer; a form the oracle does not evaluate is counted unchecked, never guessed |
| A7 Terraform names where an override retypes a variable declared as a set (#740) | new | the maintainer's table on #740 (2026-11-09): Terraform v1.17.1's names per root, `rejected` where Terraform refuses the configuration | 4 | `prove.py iac` over `a7-tf-set` (`terraform-names.tsv`) | the maintainer, from Terraform's `console`; never Meada |
| A7 IaC nodes, CloudFormation and SAM (`search` kind `iac`, `context` and `trace` on a logical id, evaluated names and patterns, `overridable:`, `NoEcho`) | new | ticket #653's criteria and CloudFormation's own rules, through `cfn_oracle.py`'s reading of the files (PyYAML with a constructor per tag) | 4 | `prove.py iac` | the maintainer; the oracle reads the files, never Meada |
| Every other answer of an A7 entry, and a repository with no config | existing | the base binary on the same copy | 1 | `prove.py values` (status, export, default search), `prove.py run` | the code before the change |
| A7 orphan status and side markers, group isolation, visibility and refresh | new | #657 and ADR 0026 at the brief's base; HTTP copies stdio in addition | 4 + 1 | `orphans.py --http`, neutral generated inputs with SHA256 inventory | the maintainer; generated inputs are independent of feature tests |
| A7 config use sides, typed final keys, directionless provision links, key sites and orphan uses | new | #661/#642 and base ADR 0026; independently generated source-derived whole sets; HTTP/console also copy stdio/CLI | 4 + 1 | `config_uses.py`, [references/config-uses.md](../prove-meada/references/config-uses.md) | maintainer spec; fresh independent checker |

## A7: config values and IaC (spec #642)

**Drive**: `prove.py values --http` over the A7 entries (and `prove.py iac --http` for the IaC nodes), beside `prove.py run --http` on the corpus entries Scope names: the command, the pinned repositories, the story-by-story map and what could fool it are in [references/config-iac.md](../prove-meada/references/config-iac.md).

**Copies**: new flows answer to #642's user stories. The YAML/JSON oracle reads its supported config files itself; native HCL and generic IaC use their separate frozen source keys and whole-output judgment. HTTP copies stdio. What an A7 change leaves alone answers to the base binary.

**Done looks like**: `values.md` shows per entry the status config line equal to the oracle (absent on `a7-noconfig`), the other status lines and the export equal to base, every value query equal to the oracle line by line, and "N/N answers byte-equal to stdio" over HTTP; stories 14 and 16 judged by the checker, 15 under Not covered.

**Could fool you**: an export byte-identical base to head over inputs that lack the changed construct is no evidence (#754's run 42931561229: CAPTURED, judged UNPROVEN, since no input declared a set): give the capture one first (`../prove-meada/references/scope.md`, "An input for a new flow"); one value per file is sampled; a flow of the A7 table still `pending` has no check (UNPROVEN); the oracle's own conventions are questions for the maintainer, never a fix to the oracle. The full list: the reference's "Could fool you".

## Flows

For Terraform/OpenTofu JSON, run `python3 $S/terraform-json.py --head-bin "$RUN/bin/meada-head" --run "$RUN/terraform-json" --http`. Judge the complete node/provisioner/subscription sets, marker privacy, source spans/references, warm/cold refresh and source-deletion behavior against #642, #649 and HCL JSON syntax; HTTP copies restored stdio. [references/terraform-json.md](../prove-meada/references/terraform-json.md) fixes the neutral input inventory and limitations.

For #657's orphans, run `python3 $S/orphans.py --base-bin "$RUN/bin/meada-base" --head-bin "$RUN/bin/meada-head" --run "$RUN/orphans" --http`; judge `orphans.md`, every whole status capture and both transport captures against #657 and base ADR 0026. Preserve declared-resource writer/reader diagnostics separately. Shared query changes also require all corpus entries and console API parity.

For #661 config use sides, run `python3 $S/config_uses.py --base-bin "$RUN/bin/meada-pre-config" --head-bin "$RUN/bin/meada-head-ui" --run "$RUN/config-uses" --http --ui-dir ui/dist`. Complete source-derived use/provisioning/orphan sets answer to #661 and base ADR 0026; every old export field answers to base, HTTP/console to stdio/CLI. Same-size remove/restore, filtered scopes, response mutation controls and visible graph/query checks are required: [references/config-uses.md](../prove-meada/references/config-uses.md). A run without the built console leaves that surface unproven.
