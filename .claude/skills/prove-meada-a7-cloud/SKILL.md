---
name: prove-meada-a7-cloud
description: "Meada proof flows for AWS, GCP and Azure resources and SDK sides (A7). Read after /prove-meada."
disable-model-invocation: true
---

# Prove cloud resources and SDK sides (A7)

Read `.claude/skills/prove-meada/SKILL.md` first: it owns Scope, Sort, the answer-key rules, the verdicts and the receipt, and it sets `S`, `RUN` and `BASE`. Every A7 flow also follows the **Copies** and **Could fool you** paragraphs of [prove-meada-a7-config](../prove-meada-a7-config/SKILL.md).

## Answer keys

| Flow or surface | Flow kind | Answer key | Rung | Where it lives | Written by |
|---|---|---|---|---|---|
| A7 AWS type mapping, ARN extraction, SecureString, subscriptions, scope widening | new | #658/#642 and ADR 0025/0026 at the base; independently authored neutral workspace | 4 | `aws.py`, [references/aws.md](../prove-meada/references/aws.md); HTTP also answers to stdio (1) | maintainer spec; independent checker |
| A7 Java 2.x, Go v2 and .NET AWS SDK forms, constructor evidence, exact hops, hidden names, environment provenance | new | #660/#642, base ADR0025/26, independently generated source; coordinating brief supplies operation inventory missing from ADR | 4 | `aws-sdk-jvm-go-dotnet.py`, [references/aws-sdk-jvm-go-dotnet.md](../prove-meada/references/aws-sdk-jvm-go-dotnet.md); HTTP additionally copies stdio (1) | maintainer spec and independent checker; explicit brief inventory |
| A7 Azure SDK forms in JS/TS, Python, Java, Go and C#, exact named receivers, canonical selectors and scoped subscription companions | new | #670/#642, ADR0026 and official SDK inventory; generated neutral sources and source-derived whole side sets | 4 | `azure-sdk.py`, [references/azure-sdk.md](../prove-meada/references/azure-sdk.md); HTTP additionally copies stdio (1) | maintainer spec, official SDK research and fresh checker |
| A7 GCP SDK forms in JS/TS, Python, Java, Go and C#, constructor and named-handle evidence, subscription subtype and scoped environments | new | #668/#642, base ADR 0026, official SDK samples and coordinating inventory; generated source predates product answers | 4 | `gcp-sdk.py`, [references/gcp-sdk.md](../prove-meada/references/gcp-sdk.md); HTTP additionally copies stdio (1), existing flows answer to exact base | maintainer spec, official SDK research and independent checker |
| A7 JavaScript v3/TypeScript and boto3 SDK sides, origins and hidden names | new, plus existing SQS controls | #659/#642 and base ADR 0025/0026; exact base on identical SQS input; HTTP/console copy stdio/CLI | 4 + 1 | `aws-sdk.py`, [references/aws-sdk.md](../prove-meada/references/aws-sdk.md), neutral pre-answer oracle and complete exported side/link sets | maintainer spec; independent checker; base binary |

## Flows

For #667 GCP resources and Cloud Run, run `python3 $S/gcp.py --head-bin "$RUN/bin/meada-head" --run "$RUN/gcp"` and inspect complete identities, subscription filters, workload scopes and transport answers ([references/gcp.md](../prove-meada/references/gcp.md)).

For #670 Azure SDK sides, run `python3 $S/azure-sdk.py --head-bin "$RUN/bin/meada-head" --run "$RUN/azure-sdk" --http`. Judge the entire five-group inventory, both JS/TS sources, exact factory frames, refusal controls, canonical identities, filtered delivery, scoped companions and refresh against #670/#642 and ADR0026. [references/azure-sdk.md](../prove-meada/references/azure-sdk.md) fixes mandatory supplementary probes and private-identity/credential limits; the script is same-agent scaffolding, never independent acceptance. Preserve #669 Azure, GCP/AWS, native C# and workload environment flows with their existing answer keys.

For #668 GCP SDK sides, run `python3 $S/gcp-sdk.py --head-bin "$RUN/bin/meada-head" --run "$RUN/gcp-sdk" --http`. Judge complete sides, provision links, subscription delivery, refusal controls, environment provenance and refresh against #668/#642 and the official inventory. [references/gcp-sdk.md](../prove-meada/references/gcp-sdk.md) names mandatory additional exact-frame, same-file scope, typed-helper and workload probes; the script alone is a minimum. Preserve existing AWS/native brokers and #667/#663 through their flows and exact-base controls.

For #660's Java, Go and C# AWS SDK use sides, run `python3 $S/aws-sdk-jvm-go-dotnet.py --head-bin "$RUN/bin/meada-head" --run "$RUN/aws-sdk" --http`; inspect whole side/provision sets, controls, environment provenance and every raw transport answer. [references/aws-sdk-jvm-go-dotnet.md](../prove-meada/references/aws-sdk-jvm-go-dotnet.md) fixes the neutral input inventory and limits; all-corpus base parity and console viewer checks also apply to the Cargo version bump.
