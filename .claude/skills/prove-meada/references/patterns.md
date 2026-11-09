# Anchored names (#674)

This extends the existing app workflow, not a migration. Answer keys are ticket #674,
parent #642 and ADR 0026 at the base commit. The driver generates its own neutral Terraform,
SDK, declaration, Compose and workflow inputs, never importing feature fixtures or goldens.
It retains input hashes, complete exports, CLI choices, stale/restore responses and raw MCP
answers. An independent checker must review its assertions against those answer keys.

Run from the repository root with the exact built head binary copied while holding the
shared Cargo lock:

```bash
python3 .claude/skills/prove-meada/scripts/patterns.py \
  --head-bin "$RUN/bin/meada-head" --run "$RUN/patterns" --http
```

The driver checks complete provisioning sets, inferred channel/resource/image matches,
multiple literal candidates without choosing one, minimum literal evidence, short and
interior wildcard rejection, no pattern-to-pattern link, a real `meada decide` human choice,
its replay into export/context/trace, a stale renamed target, restoration, and equality
between stdio and HTTP answers. It writes `checks.md` only after every assertion passes.

The script also exercises reverse literal/pattern orientation, indexed config-key locations
for choices, and builder-pattern image choices. The script alone is not the final verdict. Check independent evaluation roots
against the spec too; preserve existing NATS and RabbitMQ
routing semantics. The existing `images.py --http --public` flow with its UI options drives
the console graph and image ambiguity. Shared engine/decision changes also run existing
code-only corpus and decision controls against the base binary. A missing surface remains
UNPROVEN, never silently omitted.

Check retained runner choices across builder-name selections too: when one remaining builder
owns unresolved east/west ambiguity, an old runner choice becomes `item-not-open`. When that
runner still has two literal builders, a pattern selected for east cannot appear among its
west candidates or replay through a literal at the same repository site; that target is
`not-candidate`. Assert the complete candidate and link sets in both transitions.

The same agent supplied the driver and feature. The parent task arranges independent final
proof and records its receipt in the PR's Evidence section. Failed assertions are BROKEN;
a passing run without that independent review is implementation validation only.
