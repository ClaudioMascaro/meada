# HTTP routes declared in IaC (#666)

Answer key: #666 and #642, ADR 0005/0025/0026, plus the exact pre-change binary for the
existing code HTTP control. This extends the existing app skill, not a migration skill.
The neutral sources and complete expected sets are written before the first indexing call;
fixture goldens and observed exports never supply the answer key.

```bash
python3 $S/iac-http-routes.py --head-bin "$RUN/bin/meada-head" \
  --base-bin "$RUN/bin/meada-base" --run "$RUN/iac-http-routes"
```

The source-derived inventory has five providers and five consumer links: SAM Api/HttpApi,
Serverless httpApi, Terraform API Gateway HTTP/Lambda, and an ordinary code route. The SAM
handler is an export alias, with a same-name decoy elsewhere. Every provider and link is
checked, including level, method, path and handler. Wrong method/path/host, a catch-all,
a path masquerading as a method and a conditional with two Lambda targets stay absent.
An opaque archive preserves literal routes at the repository site and incoming links in
that repository's export. Restoration must restore the original exact link set.

CLI exports drive the whole graph; MCP stdio and HTTP queries must agree. Delete all three
IaC sources and every IaC handler/decoy source after server-mode indexing, then repeat
read-only HTTP queries and every repo's
provider/link export: all remain identical, including the aliased handler. The standalone
code HTTP control keeps every baseline export field and its context bytes. Source hashes,
oracles, raw MCP wire logs and complete exports remain in the run directory. Only a local
loopback listener is started, and it is stopped in `finally`.

| Method | Surface | Answer key | Catches | Misses | Setup / run |
|---|---|---|---|---|---|
| Complete source-derived sets | CLI export | 4: #666/#642 and ADRs | Extra/missing routes, wrong handlers/levels, fake targets and fallback | Other cloud formats beyond the finite inventory | none / seconds |
| Existing-byte parity | CLI export, MCP context | 1: exact base binary | Shared index/HTTP regressions on code routes | Unrelated app flows | supplied base / seconds |
| Transport and stored-fact parity | MCP stdio/HTTP, read-only export | 1: same graph and query | Partial hydration, source reads at query time, copied-surface drift | Browser rendering | none / seconds |

These methods are already approved by #642 Testing Decisions and the continuation brief.
The focused Engine tests cover the broader native/JSON, REST, quick-create and adjacent
fixture inventory. Audit partial/every-row equivalence by making the partial
`symbols_read` request written handler names without resolving stored aliases: direct controls
pass, then the aliased handler query must fail. A second audit requests no handler symbols. Full corpus, UI, docs and deploy
integration remain public CI responsibilities; this bounded receipt is not the whole release.
