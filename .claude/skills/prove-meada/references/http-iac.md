# HTTP through Services and Ingress (#665)

Answer key: #665 and #642's HTTP stories, ADR 0026 at the ticket's base. The source-generated
workspace uses independent names and routes; fixture goldens are not its oracle.

```bash
python3 $S/http-iac.py --head-bin "$RUN/bin/meada-head" \
  --base-bin "$RUN/bin/meada-base" --run "$RUN/http-iac"
```

The checker inspects the full export, requiring 13 HTTP links and one incoming image link.
It covers all four Kubernetes DNS forms, with and without ports, a workload base, a literal
Backstage proxy configuration base, an external Ingress prefix, a direct-host control and a Compose
service with a direct local code path. Every routed link is inferred and names its Service;
the direct-host link stays exact. The local code path selects the inventory handler even
though another directory serves the same route.

Break-it cases include a rewrite annotation, a longer Ingress prefix with an unknown backend,
a wrong namespace, a missing Service and a Secret base. They create no HTTP link. The verifier's
process environment deliberately disagrees with the checked-in workload entry.

CLI exports and four queries are captured, then MCP stdio and HTTP answers must match. Delete
every workload/Compose/config source supplying new routing facts and repeat the HTTP queries
against the read-only workspace: all answers and exported links must remain identical. Code
sources remain present for unchanged line numbers and context snippets; they supply no new
infrastructure facts. Logs, raw transport replies, source hashes and complete exports remain
in the run directory. The loopback listener is stopped in `finally`.

Existing non-link, non-process export fields and the direct-host link/process compare against
the baseline binary when supplied. New process hops must match the source-derived links. Existing gateway and A8 peer flows remain their own Scope rows and must run
when HTTP shared code changes. Full corpus integration belongs in public CI under AGENTS.md;
a local run of this bounded flow is a scoped proof, not the whole release proof. The Proof
workflow's `prove-meada` job runs this flow on every CLI run with the merge-base binary
(`scripts/prove-ci`) and retains it under `flows/http-iac` of its artifact: `binary.json`, raw
MCP captures and source-deleted exports.

| Method | Surface | Answer key | Catches | Misses | Setup / run |
|---|---|---|---|---|---|
| Exact source-derived link set | CLI export | 4: #665/#642, base ADR 0026 | Wrong Service, provider, level, extra links, namespace and direct-path leaks | Other HTTP libraries' extraction | none / seconds |
| Existing-byte parity | CLI export | 1: base binary | Unrequested side/provider/metadata or direct-host changes | Requested new routing | supplied base / seconds |
| Copied-surface and stored-fact parity | MCP stdio/HTTP, read-only export | 1: same query and graph | Missing prepared routing, query-time IaC reads, transport mismatch | Browser graph layout | none / seconds |

These methods were already approved in #642 Testing Decisions and the continuation brief.
Mutation audit: omitting `workspace_ready` only from the answer-sized query branch
must fail the `http_iac` every-row equivalence oracle. Disabling Ingress routing or permitting
rewrites must fail the full-link source oracle. Retain the actual failing assertion in evidence.
