---
name: prove-meada-a9-ci
description: "Meada proof flows for CI in the graph (A9, spec #820). Read after /prove-meada."
disable-model-invocation: true
---

# Prove CI in the graph (A9, spec #820)

Read `.claude/skills/prove-meada/SKILL.md` first: it owns Scope, Sort, the answer-key rules, the verdicts and the receipt, and it sets `S`, `RUN` and `BASE`.

## Answer keys

| Flow or surface | Flow kind | Answer key | Rung | Where it lives | Written by |
|---|---|---|---|---|---|
| A9 CI nodes, the CI contract with its Git ref and keys, `trace` through an intermediate pipeline, external files, orphans, the absent mark and `ref mismatch` | new | #820 and ADR 0028, through `ci.py`'s own reading of the workspaces it generates, a control per case; HTTP also copies stdio | 4 + 1 | `ci.py`, [references/ci.md](../prove-meada/references/ci.md) | the maintainer; the driver reads the files, never Meada |
| A9: the files the CI reader owns, `uses` lines as config values, `[[resource]] via`, every other row of the A9 workspaces and the pinned CI corpus | existing | the base binary on the same input; allowed: the differences #820 predicts | 1 | `ci.py`, `ci.py --corpus` | the code before the change |
| A9 F2: the command table and the gate of a `run` script, the unresolved parts, programs of the workspace with their level, the fork probe (#850), `trace` across the three contracts of the chain | new | #820's tables, with the levels ADR 0029 amends (#850), through `ci.py`'s own reading of the scripts and manifests it generates, a control per case; HTTP also copies stdio | 4 + 1 | `ci.py --slice f2`, [references/ci.md](../prove-meada/references/ci.md) | the maintainer; the driver reads the files, never Meada |
| A9 F2: the image builder forms of today and the SDK's HTTP link, every other row of the `run` and `chain` workspaces | existing | the base binary on the same input; allowed: a builder's site becomes its CI step, and the rows #820 adds on CI nodes | 1 | `ci.py --slice f2` | the code before the change |
| A9 F3: declared permissions on a consumer and `permissions mismatch` on a link | new | #820's rules, through `ci.py`'s own reading of the `permissions` blocks it generates, a control per case; HTTP also copies stdio | 4 + 1 | `ci.py --slice f3`, [references/ci.md](../prove-meada/references/ci.md) | the maintainer; the driver reads the files, never Meada |

## A9: CI in the graph (spec #820)

**Drive**: `$S/ci.py --head-bin "$RUN/bin/meada-head" --base-bin "$RUN/bin/meada-base" --run "$RUN/ci" --http`: it generates the neutral workspaces of one slice under `github.com/acme`, indexes and exports them with both binaries and checks each case of its `CASES` table. The default slice is F1 (the reproduction of #816, one case per form of `uses`, Git refs, orphans, a repository without `origin`); `--slice f2 --run "$RUN/ci-run"` drives the `run` scripts and the chain of three contracts, `--slice f3 --run "$RUN/ci-permissions"` the declared permissions. `ci.py --corpus` drives set P, the three pinned repositories of flow `ci` in `$S/infra.tsv`, alone and as one workspace, then each set of the A9 yardstick (#904) as its own workspace on the head binary, saved in `sets/<set>/` for its verifier. The cases, the keys and what could fool it: [references/ci.md](../prove-meada/references/ci.md).

**Copies**: nothing for the CI rows, which answer to #820. Every other row answers to the base binary, MCP over HTTP to stdio.

**Done looks like**: for each command, `checks.json` with every check passed and `receipt.md` saying so; from `--corpus`, also `workspace/ci-links.json` and `sets/<set>/`.

**Could fool you**: a case proves something only beside its control, so read a failure as the pair; `meada status` is saved as a diff against the base and read by hand. The full list: the reference's "Could fool you".
