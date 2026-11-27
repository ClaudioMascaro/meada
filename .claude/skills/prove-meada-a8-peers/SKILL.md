---
name: prove-meada-a8-peers
description: "Meada proof flows for base URLs across peer workspaces (A8, spec #683). Read after /prove-meada."
disable-model-invocation: true
---

# Prove base URLs across peer workspaces (A8, spec #683)

Read `.claude/skills/prove-meada/SKILL.md` first: it owns Scope, Sort, the answer-key rules, the verdicts and the receipt, and it sets `S`, `RUN` and `BASE`.

## Answer keys

| Flow or surface | Flow kind | Answer key | Rung | Where it lives | Written by |
|---|---|---|---|---|---|
| A8 base URL cases (`a8-cases.tsv`, `landed`) | new | spec #683's expected lines (#684's table) | 4 | `a8-peers.sh check` | the maintainer |
| The 5 A8 controls and every other field of the peer workspaces' exports | existing | the base binary on the same generated workspaces | 1 | `a8-peers.sh check` | the code before the change |
| g9's Go `type` rows (#973) | existing, touched | #973's rule ("a `type` row from each function, method, type or variable to each type it names") over g9's generated Go source | 4 | `a8-expected.jq` (`g9_type_uses`) | the maintainer (#973) |
| Communities of a workspace whose calls a spec changes (g9, while the base predates #973) | existing, touched | spec #31's rules over the expected calls: members are the call symbols, one community each, names by `src/community.rs`'s rule, connected | 4 | `a8-communities.jq` | spec #31 |

## A8: base URLs across peer workspaces (spec #683)

**Drive**: `$S/a8-peers.sh check "$RUN/bin/meada-base" "$RUN/bin/meada-head" "$RUN/peers"`: it generates neutral sibling repositories under `example.com/acme` (two peers serving `GET /orders`, one behind the Backstage proxy, one consumer per case shape), indexes and exports them with both binaries and compares each case of `$S/a8-cases.tsv`. The workspaces, the case states and what could fool it: [references/peers.md](../prove-meada/references/peers.md).

**Copies**: nothing. A `landed` case answers to #683, a control and every other export field to the base binary.

**Done looks like**: `peers.md` with every control and landed case `PASS`, every `pending` case `PENDING (as 1.2.0, not counted)` and named so on the receipt, and every workspace "equal to base".

**Could fool you**: a `pending` case passes on 1.2.0's value, so it proves only that nothing moved it; the script reads the export, not the MCP answers. While the base predates #973, g9's communities are checked by property, not byte for byte: a regrouping that keeps every group connected and well named passes. The full list: the reference's "Could fool you".

The public smoke flows take the source keys and pinned revisions from
[the smoke answer key](../prove-meada/scripts/smoke-stack/README.md). They keep
each workspace and each binary upgrade independent. Run and judge the exact
HTTP/SQS cases, lifecycle, warm/cold upgrade and authored controls separately.
