---
name: prove-meada-deploy
description: "Meada proof flows for the deployment files (Compose, Helm, the container image). Read after /prove-meada."
disable-model-invocation: true
---

# Prove the deployment files

Read `.claude/skills/prove-meada/SKILL.md` first: it owns Scope, Sort, the answer-key rules, the verdicts and the receipt, and it sets `S`, `RUN` and `BASE`.

## Answer keys

| Flow or surface | Flow kind | Answer key | Rung | Where it lives | Written by |
|---|---|---|---|---|---|
| Compose and Helm renders | existing | the renders at the base | 1 | `prove.py deploy` | the deploy files before the change |

## Deploy

**CI drive**: `python3 $S/prove.py deploy --base "$BASE" --run "$RUN"`, then `python3 scripts/compose-check` and, with `helm` installed, `python3 scripts/helm-check`. Detail: [references/deploy.md](../prove-meada/references/deploy.md).

**Copies**: nothing. The renders at the base are the answer key; each difference must be one the issue names.

**Done looks like**: every render identical or its difference predicted, both check scripts exit 0.

**Could fool you**:
- A render proves the configuration, not that the containers start; `scripts/container-smoke` does that, and needs Docker and an image build.
- A render shows a mounted file's path, never its text: `prove.py deploy` copies the Caddyfiles, workspace manifests, shell scripts and env examples the stack mounts into the render folder (`mounted-*`) so the base/head diff covers them. `compose-check` asserts only a few strings of those files (a Caddyfile upstream on the wrong port passed it, audit 2026-11-03, D3); read the `mounted-*` diff as you read a render's.
