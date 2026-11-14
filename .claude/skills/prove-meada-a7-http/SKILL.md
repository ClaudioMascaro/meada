---
name: prove-meada-a7-http
description: "Meada proof flows for HTTP through Services, Compose and Ingress and for IaC-declared routes (A7). Read after /prove-meada."
disable-model-invocation: true
---

# Prove HTTP through infrastructure (A7)

Read `.claude/skills/prove-meada/SKILL.md` first: it owns Scope, Sort, the answer-key rules, the verdicts and the receipt, and it sets `S`, `RUN` and `BASE`. Every A7 flow also follows the **Copies** and **Could fool you** paragraphs of [prove-meada-a7-config](../prove-meada-a7-config/SKILL.md).

## Answer keys

| Flow or surface | Flow kind | Answer key | Rung | Where it lives | Written by |
|---|---|---|---|---|---|
| A7 HTTP through Service DNS, Compose names and Ingress prefixes | new, plus existing direct-host/gateway/A8 controls | #665/#642, base ADR 0026 and independently named source; existing bytes against base; MCP HTTP copies stdio | 4 + 1 | `http-iac.py`, [references/http-iac.md](../prove-meada/references/http-iac.md), complete link/process sets and source-deleted parity | maintainer spec; fresh checker required |
| A7 IaC-declared HTTP routes, exact handlers and repository fallback | new, plus existing code HTTP control | #666/#642, ADR 0005/0025/0026 and complete source-derived provider/link sets; baseline code HTTP bytes and transport parity | 4 + 1 | `iac-http-routes.py`, [references/iac-http-routes.md](../prove-meada/references/iac-http-routes.md), neutral pre-answer oracle, alias/fallback and source-deleted checks | maintainer spec; fresh independent checker required |

## Flows

| Flow | Stories | Surfaces | Entries (`$S/infra.tsv`, `$S/corpus.tsv`) | Check | State |
|---|---|---|---|---|---|
| HTTP through Services, Compose and Ingress | #665; HTTP stories and ADR 0026 | CLI export, MCP stdio/HTTP and read-only stored queries | neutral source-generated workload/provider/Service workspace, whole-link oracle and source-deleted parity | `http-iac.py --head-bin BIN --base-bin BASE --run DIR` ([http-iac.md](../prove-meada/references/http-iac.md)) | #665 |
| HTTP routes declared in IaC: exact finite methods/paths, same-repository handlers or repository fallback | #666; #642 HTTP stories; ADR 0005/0025/0026 | CLI export, MCP stdio/HTTP and stored read-only queries | neutral SAM/Serverless/Terraform/code/consumer workspace, complete provider/link oracle and adjacent misses | `iac-http-routes.py --head-bin BIN --base-bin BASE --run DIR` ([iac-http-routes.md](../prove-meada/references/iac-http-routes.md)) | #666 |
| HTTP providers from IaC-declared API Gateway, SAM and Serverless routes | 52 (Services/Ingress above) | CLI export, MCP | neutral #666 sources, with the public `sam-rust` source subset judged separately | registered `iac-http-routes.py` row above and its independent complete provider/link key | landed (#666); public source capture is not an independent verdict |

For #665 HTTP routing, run `python3 $S/http-iac.py --head-bin "$RUN/bin/meada-head" --base-bin "$RUN/bin/meada-base" --run "$RUN/http-iac"`. Inspect the complete source-derived link set, namespace/path/rewrite controls, raw transport replies and source-deleted read-only parity ([references/http-iac.md](../prove-meada/references/http-iac.md)). Existing gateway and A8 controls remain required for shared HTTP changes.
