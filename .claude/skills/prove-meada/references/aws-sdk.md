# A7 AWS SDK use sides (#659)

This extends the permanent app skill through `/create-prove-it`. CLI export/status and MCP
stdio are primary; MCP HTTP copies stdio; the console query/export API copies MCP/CLI and its
existing graph tables copy that export. No new UI representation is required. The answer keys
are maintainer #659/#642 and ADR 0025/0026 at the brief's base for new flows, the exact base
binary on identical code-only SQS inputs for existing flows, and parity for copied surfaces.
Read the issues and comments at run time. Feature fixtures, feature tests, changed goldens and
the implementation are never the answer key.

The AFK dispatch already selects an independent checker and the app, rather than a migration.
Use these methods without an additional confirmation round:

| Method | Surface | Answer key | Catches | Misses | Setup / run | Reachable |
| --- | --- | --- | --- | --- | --- | --- |
| Independently authored operation matrix and exact side/link sets | CLI export/status, MCP context/trace | 4: maintainer spec and base ADRs | Missing/wrong operations, direction, literal identities, provisioners, extra sides and links, unresolved environment names | Other language SDKs | none / seconds | supplied binaries |
| Origin and hidden-name refusals | CLI export | 4: #659 | Name-matched factory hops, unknown constructors, TS/JS parameter/catch/destructuring and Python rebinding false positives | Every possible syntax spelling | none / seconds | supplied binary |
| Existing SQS/code parity | CLI export/status, MCP | 1: exact authorized base binary | Shared-code regressions on existing forms | Other existing flows, covered by Scope's corpus | serial base build / seconds | supplied base |
| Full query/error transport parity | MCP HTTP and console API | 1: head stdio, plus spec above | Missing server rows, partial answers, changed refusal protocol | Shared mistakes on both copies | loopback / seconds | supplied binaries |
| Loaded graph table/export parity | Console browser | 1: complete CLI/API export | Missing contract, side, provisioning table or graph data; stale UI | New UI design, none requested | cached Chromium/Playwright / seconds | fresh UI binary and frontend |
| Layout shift | Console graph | budget: CLS <= 0.1 | Unstable graph layout | Visual taste | same browser / seconds | same |
| Load time | Console graph | budget: <= 2 s on loopback | Startup/page regression | WAN performance | same / seconds | same |
| Accessibility | Console graph | budget: zero serious/critical axe violations | Inaccessible loaded table | Manual assistive-device use | same / seconds | same |
| Console errors | Console graph | budget: zero errors | Broken WASM/parser/page | Silently wrong data, checked by parity | same / seconds | same |

## Drive

Source the session's tool and service env, then use the coordinator's serially built binaries:

```bash
source /workspace/meada-setup/env.sh
source /workspace/meada-setup/services.env
python3 .claude/skills/prove-meada/scripts/aws-sdk.py \
  --head-bin "$RUN/bin/meada-head" --base-bin "$RUN/bin/meada-base" \
  --ui-bin "$RUN/bin/meada-head-ui" --ui-dir ui/dist --run "$RUN/aws-sdk"
```

The new run directory is required. `oracle.json` and its source-file SHA256 inventory are saved
before any Meada answer. The script initializes isolated Git repositories, never indexes the
checkout itself, and records raw CLI stdout/stderr/exits, stdio wire captures, HTTP/API answers,
complete browser table captures, screenshots, build hashes and each check as it happens.
Read the supplied binaries' `.from` files and preserve them beside the receipt. A setup binary
may help establish the pre-existing wire shape but is never evidence for exact-base parity.
Do not build concurrently with the implementation agent. No cloud SDK code is executed and
no cloud API, credential or account is required.

## Done looks like

The complete literal matrix runs in TypeScript, JavaScript and Python, including import aliases:
SNS Publish; EventBridge PutEvents; Kinesis PutRecord/PutRecords; S3 PutObject/GetObject;
DynamoDB PutItem/UpdateItem/DeleteItem/BatchWriteItem/TransactWriteItems writers and
GetItem/BatchGetItem/Query/Scan/TransactGetItems readers, with a separate
TransactWriteItems ConditionCheck reader; SSM GetParameter/GetParameters readers,
GetParametersByPath unresolved hierarchy readers and Secrets Manager
GetSecretValue/BatchGetSecretValue readers. Nested entries, request maps and transaction entries
carry the name in the SDK's low-level input shape. These are independent ordinary SDK cases,
not a claim that #659's illustrative operation list limits support to these spellings.

Every expected side has its exact role, contract and `via` provenance: `sdk:aws` for a literal
SDK origin and `env:<variable>` for each environment name. The complete provider, consumer and
link arrays are checked, so an unexpected contract of any kind also fails. The side count, provisioning set,
reader-to-writer set and levels agree. Every resolved literal joins one matching provisioner; every
service has one unresolved environment side per language, whose context names its variable,
with no provisioning link. All 21 variables are explicitly set to matching physical names in
the indexer's process environment, making a forbidden runtime lookup observable. Every
supported resolved contract appears in IaC context, downward trace and
upward trace. Unknown constructors, factory results reached by a name match and every generated
hidden binding contribute zero SDK side. The seven IaC provisioners have no orphan status row.
The old SQS control's complete export/status and MCP answers equal the authorized base.

All queries, including the unknown-kind error, agree over stdio, HTTP and console API. Console
exports equal CLI bytes on the same workspace/repository. The browser actually loads app,
peer and infrastructure exports and compares every contracts/provisions/links table cell, including
all list items, to those exports. Both provisioners and use-side links must survive the existing
graph representation. The four existing page budgets pass. Servers stop by their own PIDs,
and all evidence survives cleanup.

An SSM hierarchy Path is never an exact physical-parameter name: its literal contributes an
unresolved reader without a provisioning link. ConditionCheck reads an item even though the
enclosing operation is called TransactWriteItems. These semantic branches were added to the
independent oracle before any head answer, with the original draft and a correction note
retained in the first run's provenance. The 88 expected sides comprise 64 resolved literals,
21 environment names and three unresolved hierarchy reads; 34 hidden/origin controls add none,
including a real SDK client paired with an operation from another service in each language.
The independent peer repository supplies an S3 SDK writer: all 64 provisioning links and 282
reader/writer links must match, including three cross-repository S3 links. The browser's app
links table must therefore be nonempty, and every row still has to match the CLI/API export.
The name-matched-hop control calls a class method on an unknown receiver. The exact base
must first demonstrate inferred/ambiguous resolution; an exact local factory call does not
exercise this refusal rule and must not be labelled a name-only hop.

Exports are scoped to repository endpoints. In this neutral workspace, infra owns all 64
provisioning links and app owns all 282 reader/writer links; the script checks these complete
global arrays, then every exact app/peer/infra projection, preserving duplicate counts. It must
not assume every repository exports identical arrays. The dense DynamoDB downward trace needs
32000 tokens to include all 33 expected sides; an explicitly truncated response cannot prove
absence. The graph view picker is `.graph-view-picker select`, since a separate language picker
also uses `select`. Preserve raw failed attempts when correcting transport/selector mechanics.

## Judgment and failure handling

Exit 0 means checks passed, never an automatic PROVED verdict. The independent checker reads
raw evidence and writes a receipt with Scope, Sort, answer-key authors, surface-by-surface
verdicts, failed/retried steps, commands, limitations and cleanup. A mismatch is BROKEN; a
checker/runtime failure is BLOCKED; an unavailable required surface is not silently omitted.
Return findings to the implementation agent and rerun the same inputs after a product fix,
at most three rounds. Preserve every prior run and never revise an assertion to agree with
the product. Shared-code corpus proof, release/docs and required repository gates remain the
coordinator's responsibility when Scope reaches them.
