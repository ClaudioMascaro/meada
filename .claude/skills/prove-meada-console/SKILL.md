---
name: prove-meada-console
description: "Meada proof flows for the console, its API, sign-in and the infrastructure graph. Read after /prove-meada."
disable-model-invocation: true
---

# Prove the console and its API

Read `.claude/skills/prove-meada/SKILL.md` first: it owns Scope, Sort, the answer-key rules, the verdicts and the receipt, and it sets `S`, `RUN` and `BASE`.

## Answer keys

| Flow or surface | Flow kind | Answer key | Rung | Where it lives | Written by |
|---|---|---|---|---|---|
| Console query and export API | copies MCP and CLI | the head's stdio answers and `meada export` bytes | 1 | `console.py api` | the MCP server and the CLI |
| Console pages | existing | the budgets below, and the playground answer equal to stdio | budget + 1 | `console-pages.mjs` | the maintainer, 2026-11-02 |
| A7 console infrastructure roles, roots, image/alert directions, same-repository links, unpaired sides and keyboard/search controls | new | #675/#642 and ADR 0026; complete same-mode CLI/API and playground/query parity | 4 + 1 | `console-graph.py`, [references/console-graph.md](../prove-meada/references/console-graph.md) | maintainer specs; fresh independent checker |

## Console and its API

**CI drive**: [references/console.md](../prove-meada/references/console.md): a `ui` build, the front end, `console.py start`, `console.py api`, `console-pages.mjs`, `console.py stop`. With `--accounts` when Scope reaches sign-in.

**Copies**: the MCP server in the same query scope (successful text byte-equal; tool errors require the existing HTTP 422/code/full-message mapping) and the CLI in the same server mode (`GET /api/v1/repositories/{identity}/export` equals `meada export --workspace <manifest> --repo <identity>` bytes, all fields retained).

**Done looks like**: every successful query text and same-mode export byte-equal, each expected tool error with exact status/code/message; every page with its `h1`, CLS at most 0.1, load at most 2 s, zero console errors, zero axe violations of impact serious or critical; the playground answer equal to stdio; with accounts, a wrong password refused, no `Origin` refused, a token issued.

**Could fool you**: a page can render from cached state; the playground check is the one that crosses the API. A green API with an old `ui/dist` proves the API only: build the front end from this tree.

## Console graph

For #675's console graph, run `python3 $S/console-graph.py --head-bin "$RUN/bin/meada-head-ui" --ui-dir ui/dist --run "$RUN/console-graph"`. Inspect the screenshots, whole role/link tables, roots, reasons, native keyboard focus, unpaired sides and `value`/`iac`/`alert` playground answers; retain the receipt and CI artifact. [references/console-graph.md](../prove-meada/references/console-graph.md) defines inputs, answer keys, budgets, controls and cleanup. `scripts/prove-ci` runs this flow in the Proof workflow's `prove-meada` job and `proof-a7-a8-ui` runs it in the manual audit, both against a real UI binary; unavailable browser tooling leaves it BLOCKED.

## Gotchas

- The cloud modules beyond accounts (snapshot, generations, OIDC, directory, GitHub App, named workspaces) need Valkey, S3 or an identity provider: Not covered here, with the unlock `deploy/compose` or CI's `cloud` job.
