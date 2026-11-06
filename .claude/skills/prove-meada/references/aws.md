# A7 AWS channels, resources and subscriptions (#658)

This extends the permanent app skill through `/create-prove-it`, rather than introducing a
second skill. The CLI and MCP stdio are the primary surfaces. MCP HTTP copies stdio. The
answer keys for the new flows are maintainer issue #658, its parent #642 and ADR 0025/0026 at
the brief's base, never the feature's type table, tests or goldens. Fetch both issues and their
comments at run time. The issue wins if these instructions and it disagree.

## Method selection

The user requested an independent checker and a permanent A7 flow. The methods below implement
that scope; no new external service or credential is needed for indexing local repositories.

| Method | Surface | Answer key | Catches | Misses | Setup / run | Reachable |
| --- | --- | --- | --- | --- | --- | --- |
| Independent neutral input | CLI export and status, MCP context and trace | 4: #658/#642 and base ADRs, maintainer | Missing AWS identities, false provisioners, missing subscriptions, wrong filtered levels, one-way traversal, SecureString leaks, absent orphan names | SDK forms delivered by separate tickets | none / seconds | supplied local binaries |
| Transport parity | MCP HTTP | 1: the same binary over stdio, plus the spec checks above | Server-mode omissions and transport-specific answers | A mistake both transports share | ephemeral local server / seconds | loopback socket |
| Same-size edit and restore | CLI export | 4: names and subscription source must follow the edited IaC | Stale resource names or subscription links | Unmodified shapes | none / seconds | generated copy only |
| Existing-flow parity | CLI export, MCP context and trace | 1: supplied base binary on identical code-only input | Shared-code regressions on a neutral code flow | Other languages and surfaces | none / seconds | supplied base binary; corpus suite also required by Scope |

## Drive

From the repository root, after the normal build and identity checks:

```bash
python3 .claude/skills/prove-meada/scripts/aws.py \
  --head-bin "$RUN/bin/meada-head" --base-bin "$RUN/bin/meada-base" \
  --run "$RUN/aws"
```

A resource-constrained session may reuse binaries built by its coordinating agent. Record their
`.from` identity and the source/base commit in the receipt. Do not build concurrently with it.
The script requires a new run directory, creates only neutral repositories under that run's
`work/`, logs raw evidence as it is produced and stops its HTTP server by its recorded pid.
No cloud APIs are called. A secrets-manager ARN uses AWS's six-character generated ARN suffix;
SSM ARN extraction preserves its parameter name's leading slash.

The generated `oracle.json` lists every expected address/contract and source/target subscription
pair before any Meada answer is inspected. Inspect `checks.json` and the raw CLI/MCP captures;
a zero exit is only “checks passed”, and the independent checker writes the final verdict.
A verifier crash is BLOCKED, an observed spec mismatch is BROKEN. Never change an assertion to
match an observed answer. Send failures back to the implementation agent and rerun the same
flows after the app is fixed, at most three rounds.

## Done looks like

- The complete provisioner set equals the literal and ARN cases, including RDS clusters and
  ElastiCache replication groups. Unknown types, data sources, mismatched ARN services,
  unresolved whole names and the shadowed `.tf` contribute no provisioner.
- Every new kind has provisioning links, context of its IaC nodes and downward trace; resource
  read functions trace back upward. Status names unused provisioners and unprovisioned uses
  separately for every broker/kind covered by the issue.
- SNS and EventBridge consumer routes reach exactly their source producers, no extra producer.
  Unfiltered routes are exact and filtered routes inferred; reasons identify the subscription.
  Literal ARNs and IaC references work. The no-pattern/no-schedule EventBridge cases are synthetic
  unfiltered representations; a schedule-only rule does not route arbitrary bus messages.
  EventBridge's omitted bus means `default`, and a target
  on another bus cannot pick a same-named rule from the wrong bus. A positive second-bus
  route with the same physical rule name reaches exactly that bus's producer. Duplicate
  declarations of one physical bus/rule make a literal target ambiguous and yield no route.
- The seven refusal controls add no subscription link, including a schedule-only EventBridge
  rule and a duplicate physical-rule target. The export remains format 1, preserving
  distinct subscription and provisioning arrays. SecureString shows `redacted` by key path,
  cannot be found by its sentinel value, and leaks to no captured answer/export.
- Every scripted stdio answer, including empty search and unknown-kind refusal, is byte-equal
  over HTTP. A same-size topic rename removes the old provisioning and routed-producer links;
  restoring the file restores all four exported link/side arrays.
- The code-only export and MCP answers equal the base. Run the corpus entries required by the
  full diff's shared-code Scope separately, as well as docs and release checks.

## Could fool you

The neutral inputs cover Terraform/OpenTofu, not every IaC format the type table can map.
Documented AsyncAPI producers exercise the new brokers without claiming new AWS SDK support.
Declared resource readers intentionally keep their declared level. Orphan assertions require
the complete #657 status set and row count: one unused provisioner and one missing use for
each generated AWS group, plus the unprovisioned default EventBridge source. Duplicate rows,
wrong directions, extra contracts and the superseded reporter fail. Isolated one-sided groups
must have zero orphan rows. The checker still reads raw status to judge grouping and sites.
The generated names are static and hand-authored from the maintainer's spec; they do not cover
all provider aliases, resource attributes or ARN partitions. The ordinary A7 evaluator proof
and shared-code corpus remain necessary when those paths change. A console graph, grants and
actual deployment are outside this local CLI/MCP probe and must be recorded under Not covered
when the change's Scope reaches them.

## Mechanics verified in the first independent run

Repository exports are scoped: read subscription `links` from the application export that owns
its consumers, and IaC `subscriptions` from the infrastructure export. Assert whole-array
counts before checking each expected side, level and root; converting to a set alone would
hide duplicates. SQS context legitimately shows a consumer without a producer for refusal
controls. A no-matches answer echoes its query. Require the complete answer to equal exactly the
expected no-matches line, including its newline, before excluding that intentional sentinel
echo from the general absence scan. An appended row or additional secret occurrence fails.

`status` reads existing state. Explicitly index/export isolated orphan-control repositories,
assert known nonempty provisioner/use rows, then ask status; “not indexed” proves nothing.
For base status parity, ask each binary's status immediately after that binary's export,
before another version overwrites the index. The CLI can prove these controls without a
socket; HTTP needs local loopback access and records its own started and stopped process.


## Focused JSON SecureString supplement

ADR 0025 includes Terraform `.tf.json` and OpenTofu `.tofu.json`. The #658 SecureString
criterion applies to both formats. When review discovers a format omitted from the main
probe, drive this separate focused mode on the final rebuilt HEAD, without rerunning the
whole AWS flow:

```bash
python3 .claude/skills/prove-meada/scripts/aws.py --json-values \
  --head-bin "$RUN/bin/meada-head" --run "$RUN/aws-json-values"
```

Its independent generated repository has one file of each spelling. Each holds one literal
`SecureString`, one unresolved `${var.unresolved_type}` and one ordinary `String` parameter,
with distinct sentinel values. The coordinating review requires unresolved types to redact
when `SecureString` cannot be ruled out. The pre-answer oracle derives key paths and source
lines directly from the generated JSON. The CLI must count two read files and four redacted
values; export remains format 1 and contains no secret value. Twelve stdio value queries must
match complete expected answers: four hidden keys show `redacted`, their four sentinel searches
return exactly their expected no-matches line with newline, and both ordinary keys and values
remain visible. All twelve answers and the tool list must equal HTTP. The server is stopped
by its own pid; raw CLI, query, stdio and HTTP evidence stays under the new run directory.
This supplements format coverage and does not claim additional JSON IaC graph behavior.

## Shared image storage and trace supplement

When the required baseline already supports this AWS flow, also run `images-mixed.py` as
documented in [config-iac.md](config-iac.md#combined-imageaws-and-literal-ko-regressions).
It places an AWS subscription and image workloads in the same repository, preserves the
baseline's complete AWS rows, drives both trace relationships together, and edits/restores
each feature without disturbing the other. Its unresolved-registry case also checks that
static image inference does not read process environment. This supplements, never replaces,
the broad AWS and JSON SecureString modes above.
