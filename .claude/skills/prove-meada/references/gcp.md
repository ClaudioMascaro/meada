# GCP infrastructure and Cloud Run (#667)

Answer key: #667/#642 and ADR 0026's identity, subscription, level and scope rules, read at
proof time. This extends the existing app skill through create-prove-it.

```bash
python3 $S/gcp.py --head-bin "$RUN/bin/meada-head" --run "$RUN/gcp"
```

Use an immutable source-under-review binary and an absent run directory. The driver builds
nothing and accesses no cloud. It generates neutral sibling repositories, pins input bytes,
and captures CLI exits/stdout/stderr, MCP answers and server logs. Loopback is required.

| Method | Surface | Answer key | Catches | Misses | Cost |
| --- | --- | --- | --- | --- | --- |
| Complete provisioners/uses, subscription and image links, status | CLI | 4: #667/#642, generated source identities | wrong kinds, same-name subscription/topic collision, lost database instance, wrong levels, runner/orphan omissions | new SDK operations | existing binary, seconds |
| Search, each workload context, trace both ways | MCP stdio | 4: same specification and source | invisible workloads, lost environment targets and execution | code outside image scope | seconds |
| Tool and answer byte parity | MCP HTTP | 1: stdio, plus 4: specification | transport drift and dropped rows | authenticated named grants | loopback, seconds |
| Remove filter and restore | CLI refresh | 4: one source change and exact round trip | stale routes and filter bypass | arbitrary file races | seconds |

Inspect every provisioner: topic, distinct subscription, bucket, instances, same-name databases
in two instances and secret. Subscription targets are `subscriptions/<id>` and source only
the configured topic; one filtered delivery link is inferred. An unknown database instance
produces no provisioner. Context retains the physical name alongside the qualified contract;
instance defaults weaken database provisioning too.

All Cloud Run v1/v2 service and v2 job containers must be runners. Literal images are exact;
defaults inferred. Environment reads resolve only through their container's unambiguous
builder. Secrets and other-container variables remain unresolved. The consumer here uses an
established AWS SDK form; #668 owns new GCP SDK operations. Config/environment values stay
outside exports while specified use and resolved directed sides enter them.

Try same-name topic/subscription, malformed paths, unknown source/instance, equal bare values,
wrong-kind typed references, duplicate environment entries, ambiguous builders, changed
tfvars/image defaults, cut values and hostile fan-out. Audit mutations: drop the subscription
prefix, derive a database from its short name, bypass filters or share secret/container values.
The corresponding checks must turn red.

The implementer added this driver: its assertions are not an independent answer key.
`checks.json` records checks with `independent: false`; the fresh proof agent inspects raw
evidence and writes the verdict. Shared-code provision/config-use, image/environment and
pattern controls still follow Scope against the exact base. GitHub CI owns full integration
under AGENTS.md.
