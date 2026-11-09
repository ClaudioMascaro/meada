# A7 AWS SDK forms in Java, Go and C# (#660)

This extends the permanent application proof through `/create-prove-it`. It adds flows to
existing CLI/MCP surfaces. The CLI export and MCP stdio are primary; HTTP copies stdio.
The answer keys are maintainer #660 and parent #642, the indexed neutral source, and ADR
0025/0026 at the brief's exact base. Fetch those issues and base ADRs through connected
GitHub reads before checking. Feature code, its fixtures, goldens and newly written docs
are never answer keys.

The referenced ADR contains no operation inventory. The coordinating brief fixes the
operation scope below. Record that distinction in the receipt: this probe cannot claim
that an absent ADR operation table was checked. Client direction follows AWS request
semantics and #642's use-side rules.

| Service | Operations and direction |
| --- | --- |
| SQS | SendMessage, SendMessageBatch produce; ReceiveMessage consumes |
| SNS | Publish, PublishBatch produce |
| EventBridge | PutEvents entries produce |
| Kinesis | PutRecord, PutRecords produce |
| S3 | PutObject, DeleteObject write; GetObject, HeadObject, ListObjects, ListObjectsV2 read |
| DynamoDB | PutItem, UpdateItem, DeleteItem, BatchWriteItem write; GetItem, Query, Scan, BatchGetItem read |
| SSM | GetParameter, GetParameters read; GetParametersByPath is an unresolved hierarchy query; PutParameter, DeleteParameter, DeleteParameters write |
| Secrets Manager | GetSecretValue, BatchGetSecretValue read; PutSecretValue, UpdateSecret, CreateSecret, DeleteSecret write |

SDK spelling differs from logical operations: .NET S3 uses `BucketName` on all six request
models, and `GetObjectMetadataAsync(GetObjectMetadataRequest)` for logical HeadObject.
Java/Go retain Bucket/HeadObject. Java S3 PutObject includes a `RequestBody` argument.
Java imports `EventBridgeClient` and `DynamoDbClient`; .NET imports
`Amazon.EventBridge.AmazonEventBridgeClient` and `Amazon.DynamoDBv2.AmazonDynamoDBClient`.
These spellings were checked against official AWS source before product proof.

`GetParametersByPath` names a hierarchy prefix, not one physical parameter. Its literal
case must stay unresolved and link to no same-spelled parameter provisioner; its environment
case must also retain the variable. Modelling prefix candidates as inferred patterns would
need a separately specified form. This is a source-semantics correction made before any
product result was inspected.

RDS and ElastiCache data operations are outside this scope. Their control-plane SDK clients
do not stand for a data read/write. The AWS SDK service packages use Java 2.x, Go v2 and
.NET spellings. The probe never calls AWS or downloads an SDK.

## Methods and reachability

| Method | Surface | Answer key | Catches | Misses | Setup / run |
| --- | --- | --- | --- | --- | --- |
| Neutral SDK repositories with independent Terraform provisioners | CLI export; MCP search/context/up trace | 4: #660/#642, base ADRs and source; explicit coordinating operation scope | Missing or extra sides, wrong direction or name, missing exact provisioning, unresolved environment provenance, batch forms reading only the first name | SDK overloads not represented; actual runtime cloud behavior | supplied local binary / minutes |
| Type-only, hidden parameter, hidden constructor and name-matched factory controls | CLI export and call levels | 4: #660, coding standards at base | Clients accepted by type/name alone; outer constructor leaking through a hidden binding; inferred hops treated as exact | Unseeded binding forms | none / included |
| Same-size SDK name edit and restore in a persistent MCP session | MCP and CLI | 4: edited source and original complete export | Stale extraction, stale query state, broken reverse | Other edit shapes | none / seconds |
| HTTP transport parity | MCP HTTP | 1: all stdio answers from same binary, plus independent checks above | Transport omissions and tool/error differences | A bug both transports share | local loopback socket / included |
| Existing corpus parity | CLI, MCP stdio and HTTP | 1: exact base binary on identical pinned inputs | Shared-code regressions across all packs | SDK shapes absent from corpus | existing corpus cache / minutes |
| Console query/export and graph budgets | Console API and browser | 1: same-mode CLI and stdio; existing maintainer budgets | Launch/version regressions; missing browser-to-API behavior | Cloud permissions outside viewer mode | supplied UI binary and freshly built frontend / minutes |

Use every row for #660's Cargo version change. Shared helpers in `src/contract/sdk/` and the additive `Provider::via` export field also require full-corpus base parity and golden review. Binaries and services being present do not
prove reachability: start and drive them. Indexing uses generated local repositories only.
Builds/tests share the coordinating agent's serial slot, `CARGO_BUILD_JOBS=2` and
`RUST_TEST_THREADS=2`; reuse `/workspace/meada-setup/env.sh` and `services.env` when present.
No Docker is needed for this probe.

## Drive

From the repository root, after coordinated builds and binary identity checks:

```bash
source /workspace/meada-setup/env.sh
python3 .claude/skills/prove-meada/scripts/aws-sdk-jvm-go-dotnet.py \
  --head-bin "$RUN/bin/meada-head" --run "$RUN/aws-sdk" --http
```

`--run` must be a new directory. `--seed-only --run DIR` authors and hashes the neutral
inputs without driving a product binary; that mode is preparation, never product proof.
A receipt names the supplied binaries' commit/version, hash and `.from` identity. Never
index the user's checkout. The script creates only staged, uncommitted repositories under
its own run's `work/`; there are no pushes, commits or GitHub writes.

`oracle.json` and `inputs.sha256.json` precede the first product answer. Every operation has
literal, environment and type-only cases in all three languages. Every service has an exact
factory, an inferred factory, a parameter hiding a genuine outer constructor, and a local name
hiding the SDK constructor. Java also has builder-construction cases. Collection arguments
have two distinct literal names in dedicated cases, so accepting only the first fails.
The environment variables are deliberately set to tempting concrete names in the process;
their SDK sides must still stay unresolved and retain the variable, without a workload.

Inspect `checks.json`, `sdk.md`, the complete exports, CLI stderr/exit captures and all raw
stdio/HTTP answers. Each accepted literal name side must have exactly one provisioning link at
`exact`; environment sides and hierarchy-prefix sides have none. Refusal controls have no side at all. Complete side
and provisioning counts catch duplicates, extra contracts and direction swaps. An exact-hop
positive must show an `exact` factory call in the public export. A name-hop control must
actually show an `inferred` or `ambiguous` factory call: if its generated shape fails to
exercise that seam, the control is unproved, never a pass because no side happened to appear.

Every scripted new-flow context must show the literal contract or environment variable;
contract search must find each literal name; upward trace must reach its neutral provisioner at
`exact`. All queries, including missing search and invalid-kind refusal, must be byte-equal
on HTTP and stdio, with equal tool lists. The same-size S3 edit must change the side and
persistent MCP answer; restoring the bytes restores the entire export and original answer.

Existing flows answer separately to the base binary:

```bash
python3 .claude/skills/prove-meada/scripts/prove.py run \
  --base-bin "$RUN/bin/meada-base" --head-bin "$RUN/bin/meada-head" \
  --repos all --run "$RUN/corpus" --http
```

Then drive console viewer query/export parity for each corpus entry and browser budgets,
including `/graph`, using [console.md](console.md). Use the frontend freshly built from this
tree. Keep full captures; a page heading alone proves no graph data. New-form graph content
requires a feature workspace shown in the browser and compared with its same-mode export.
The generic page checks only prove the existing graph page's launch and budgets.

## Verdict and limits

A script's `checks passed` is input to the independent checker's verdict. Write the actual
receipt under `.prove-it/runs/`, using the permanent skill's template. A verifier crash,
missing binary or unreachable surface is BLOCKED with the raw error. A mismatch with an
answer key is BROKEN; the builder fixes the app. A missing answer key is UNPROVEN. Do not
relax an assertion to match the app; rerun the same flows after fixes, at most three rounds.

Constructor hidden-name controls do not enumerate every Java/C#/Go scope form. They prove the
seeded forms. Most positive methods use a local alias of a constructed client, and an exact
local factory adds one value-flow hop; field/constructor injection and every possible chain are
not covered by this neutral probe. Typed clients without a constructor are refused, except SQS,
whose declared SDK client type keeps the older forms' origin. Kotlin and non-AWS clients answer
to base parity. The console graph's new data check remains distinct from its generic page budget
check.

After inspection, stop any run-owned HTTP process still named by a pid file and remove only
this run's generated `work/` and server-index directories. Preserve oracle, raw outputs,
summary and receipt. Report leftover test data and any inadequate probe under Not covered.
