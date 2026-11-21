# Smoke checks

## Cross-repository inputs

Use `meada-smoke-*` as source repositories. Take pins and commands from the receipt, mock
PRs or tested driver. Keep public origins and sibling checkout names so Meada discovers the
peers and builds one index per repository. If the original revision predates the driver,
identify the replay's driver SHA and the gap in the original capture.

When a case lacks hosted evidence, specify the missing input, answer key and CI flow.
If implementation is in scope, put mock sources in the corpus and assertions in Meada's
driver. Propose a new sibling repo when the case needs another identity or topology.

## Choose a probe for the claim

| Claim | Exercise | Evidence to inspect |
|---|---|---|
| Peer resolution | Compare a caller with declared authority and one with two plausible providers. Reorder peers, move authority, remove and restore a route. | Join consumer, link and open item by exact call ID. Check method/path, provenance, provider, level, reason, kind and candidates. Count passing sites over total sites per binary and identify genuine ambiguities that survive. |
| Persisted state | Compare cold, warm and first export after swapping binaries on unchanged sources. Compare interrupted or concurrent operations with a clean run. | Check on-disk state and the CLI or MCP answers promised by the issue. |
| Smoke scorer | Alter a copy of a raw response. Drop a consumer but keep its link, substitute a similar caller or supply contradictory candidates. Try a repaired head with a known-bad base, then a regressed head. | Keep the original capture. Record which altered answers the driver rejects and whether it credits the recovery. Attribute these results to the scorer. |
| Driver or flow selection | Run `scripts/prove-ci --rows` once per production path that supplies the behavior, then once for an unrelated path. | Compare each selection with the Scope table and the actual reader. A combined diff that also edits the driver can select every flow and hide an omission. |
