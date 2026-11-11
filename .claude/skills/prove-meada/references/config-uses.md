# A7 config use sides (#661)

Run this flow whenever config-value classification or its callers change:

```bash
python3 $S/config_uses.py --base-bin "$RUN/bin/meada-pre-config" \
  --head-bin "$RUN/bin/meada-head-ui" --run "$RUN/config-uses" --http --ui-dir ui/dist
```

The base is a build from before #661 (`6a5eca5853193d205bcf756983b41e9cb16ffa9d`, the manual
audit's `pre-config`, which `scripts/prove-ci` builds when the flow is selected): the driver
expects that base's tool catalog, without the `alert` search kind, so the merge-base cannot serve.
The binary needs the `ui` feature for the final console checks. `--ui-dir` is optional for an
intermediate CLI/MCP run, but omitting it leaves the console unproven. Reuse the build and
browser environment from [console.md](console.md); the script never builds, downloads, or
contacts AWS. Loopback sockets must be permitted. All input repositories and server indexes
are isolated inside the new run directory, which must not already exist. Servers record
`server.pid` and `stopped`; evidence stays after cleanup. Generated Git metadata is disposable,
not a write to Meada's own Git repository, and no commits are made.

## Methods and answer keys

| Method | Surface | Answer key | Catches | Misses | Setup / run | Reachable |
|---|---|---|---|---|---|---|
| Complete source-derived use and provisioning sets, status orphan set | CLI | 4: #661, #642 stories 42, 43, 59, 63 and base ADR 0026 | wrong AWS kind/name, duplicate or missing key, wrong level/site/root, bare-name guesses, self-use of defining IaC attribute | cloud SDK additions outside #661 | existing binaries / seconds | yes |
| Every context, contract/value search and both trace directions | MCP stdio | 4: same spec and independent source; 1: base for existing value/code answers | hidden new sides, lost key locations, lost trace edge, directional leaks | grants requiring sign-in | existing binary / seconds | yes |
| All answers and tool descriptors byte parity; filtered manifest | MCP HTTP | 1: stdio on same scope, plus 4: spec | transport drift, ignored scope filter, peers read outside manifest | named workspace grants | loopback / seconds | yes with sockets |
| Full API query/export parity and browser contracts/provisions/playground | console | 1: stdio/CLI, plus 4: new source-derived uses | WASM drops new field, missing config-only contract, missing key/provision row | unrelated pages and their budgets | ui build + browser / seconds | `--ui-dir`, ui binary |
| Same-size typed-key remove/restore on persistent server | CLI/MCP refresh | 4: remove one known source fact, then exact round trip | stale uses, stale query context, overbroad refresh | arbitrary concurrent filesystem races | none / seconds | yes |
| Saved-response mutation controls | verifier | unchanged oracle rejects wrong site/key, dropped/extra use and orphan, provisioning-level corruption, HTTP key
loss and a console contract-identity substitution preserving the row count | vacuous counts or subset checks | every possible verifier defect | none / milliseconds | yes |

These methods extend the existing proof, they do not replace the shared-code corpus and
existing console budget checks selected by Scope. The coordinating checker runs those against
the same base, with all differences judged independently. No flow here has a rung-5 key.

## Independent source and coverage

The script's expected records are generated from neutral input declarations before invoking
Meada. It does not import feature tests, goldens, the type table, classifier, or rendering code.
It imports only the existing MCP transport mechanics. `oracle.json` retains every expected
contract, key, line, level, provisioner/root and provisioning link; `input.sha256.json` pins every
non-Git input byte. Every raw exit, stderr, stdout and MCP wire response is retained as produced.

The source contains all AWS ARN kinds in the base ADR's curated table (SQS, SNS, EventBridge,
Kinesis, S3, DynamoDB, RDS instance/cluster, ElastiCache cluster/replication group, SSM and
Secrets Manager), an SQS queue URL and S3 object URIs with ordinary, fragment-like and space-containing
opaque suffixes. Only the bucket establishes the resource identity. Spring YAML, Helm values and JSON cover
all four final typed keys, nested/list/quoted key paths, repeated references to the same
contract, two IaC roots (one SQS name from an overridable variable default), and a typed use without a
provisioner. An exact config side stays exact while its overridable provisioning link is
inferred and names the variable in its reason. Numeric/boolean typed values,
ancestor keys, suffix keys, bare names, unsupported ARN services, redacted Kubernetes Secret
values, unresolved templates, malformed/unsupported identifiers under typed keys, cut values,
an unreadable file and an ignored file must add nothing to the complete use set. Existing real SQS
producer/consumer and declared S3 reader/writer sides make direction mistakes visible.

A provisioner's own physical-name attribute is a declaration, not another side using its
resource. ADR 0026 distinguishes the node that creates from "every other side"; treating
`aws_s3_bucket.bucket` as a use would erase every unused-bucket orphan and contradict #642/59.
The neutral infrastructure's bucket declarations are a direct negative control. This excludes
the defining attribute, not references in the rest of an IaC file.

## Done looks like

Complete use and provisioning set equality, exact/inferred levels, precise `<file>#<key path>`
sites and all roots pass. Existing provider/consumer/link arrays and all other export fields
match base on identical generated inputs; no-config exports match completely. Config values
remain outside exports. Orphan sets have every idle provisioner and the one missing use,
without extra rows. Existing value-search answers match base completely. HTTP and console
retain every successful byte or exact error status/message. The browser visibly lists config
uses and provisioning keys and saves screenshots. Same-size removal changes live answers,
removes exactly one use; removing a provisioned key also removes exactly its two root links.
Restoring each edit restores the whole export, status and answer.

## Could fool you

- HTTP and console can agree with broken stdio. New facts need the independent spec/source
  oracle in addition to parity.
- Deduplicating by repository or contract hides repeated config keys. Check count and all
  `<file>#<key path>` locations, not just one representative.
- A narrow use-only check misses direction leaks and old links disappearing. Compare all old
  export fields with base and retain real directional siblings in the input.
- Config-only and unprovisioned uses must survive the graph model; provision-only examples
  would let a graph that ignores `uses` pass.
- A script pass says "checks passed"; the independent checker reviews raw evidence and writes
  PROVED, BROKEN, BLOCKED or UNPROVEN in its final receipt. A verifier crash is BLOCKED.
