# AWS functions and task workloads (#664)

Answer key: #664 and parent #642, ADR 0025/0026. Run:

```sh
python3 "$S/aws-workloads.py" --head-bin "$RUN/bin/meada-head" --run "$RUN/aws-workloads"
```

The driver creates its own sibling repositories and source inventory, independent of feature
fixtures and goldens. CLI export must show the complete expected SQS producer identities and
contracts, all four image runners, the two image links and all four direct executions. The
inputs include Lambda image/code/opaque ZIP/remote S3, encoded multi-container ECS, SAM
Globals/function overrides and a typed queue Ref, and a Serverless provider/function override.
The sidecar's different image must not supply an environment value to the worker repository;
code outside a direct path and NoEcho/unknown values must remain unresolved. `status` must
report the missing builder only when both image roles exist in the visible scope.

MCP stdio drives workload context and traces in both directions; MCP HTTP must return the
same complete text and tools list over an explicitly indexed workspace. Inspect the raw
captures, rather than trusting a script pass. The binary digest/version and generated input
SHA256 inventory identify the copy being driven. Every cold run starts with fresh indexes;
assertions require known nodes and sides before interpreting any absent result.

The driver mutates only a tfvars path, with unchanged Lambda configuration bytes, then tries
a raw parent component and a symlink in that path. A second source extension must make a
Serverless handler unreadable until removed. It saves the changed exports and checks the
restored producer and execution sets. The opaque ZIP is a location, never extracted code.
A process-environment canary cannot supply a name; a NoEcho canary must appear in no public
export or context. No cloud credentials or network indexing are needed. The HTTP child is
terminated in a finally block; input/source and answer captures remain with the receipt.

The script writes `checks.json`, never PROVED. The independent checker compares the whole
captures with the ticket, including scope, levels and workload/variable provenance, and records
the verdict. No live deploy, archive contents, SDK calls or Serverless plugins are exercised.
Shared contract/index parity and console checks remain in the existing Scope rows, with full
integration in GitHub CI under the current AGENTS validation policy.

Additional focused Engine controls cover curated CloudFormation/SAM provisioners, encoded
computed ECS input, unknown overrides, ambiguous workloads, path deletion/restoration, local
module/varfile changes, secret references and hostile environment fan-out. Mutation checks:
allowing symlinks or parent components, using the last workload globally, extracting archives,
or treating secret references as values must turn the corresponding public probe red.
