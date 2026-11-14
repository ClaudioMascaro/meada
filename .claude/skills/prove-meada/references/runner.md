# The runner: builds, health and clean-up

`SKILL.md` reaches this file for the mechanics of a run. GitHub Actions executes them through
`scripts/prove-ci` (the `build-*` and `clean*` steps of a receipt); the manual audit, and a flow run by
hand on a runner, follow the same commands. Preserve the same corpus, answer keys, limits and
before/after checks when arranging the CI run. `S=.claude/skills/prove-meada/scripts`, and every
command runs from the repository root.

## Start

```bash
SLUG=kotlin-extension-calls   # a few words naming the change
REPOS=meada                   # the corpus entries Scope names, comma-separated, or all
RUN=.prove-it/runs/$(date -u +%Y%m%dT%H%M%SZ)-$SLUG; mkdir -p "$RUN"
$S/build.sh head . "$RUN"           # this working tree, default features
$S/build.sh base "$BASE" "$RUN"     # the base, in a detached worktree under $RUN
```

Ready when: each line `build.sh` prints names the workspace version and what it was built from. A machine that must not build (memory, policy) may use release binaries instead, only when the base is that release's tag: [cli-mcp.md](cli-mcp.md) has the commands, and the receipt says so.

Never drive an instance you didn't start: every repository the scripts index is a copy under `$RUN/work/`, every server they start writes its pid under `$RUN`.

## Health check

`cat "$RUN"/bin/*.from` and `"$RUN/bin/meada-head" --version`: the build and its source.

## Clean up

`python3 $S/prove.py clean "$RUN"` (the corpus copies and the HTTP indexes; it stops a leftover HTTP server by its pid file), `python3 $S/console.py stop --run "$RUN"` after a console run, then `git worktree remove --force` on the build worktrees this run made (`$RUN/src-*`), never a bare `git worktree prune`: in a sandbox whose checkout is itself a linked worktree whose host path is missing, prune deletes the checkout's own worktree entry and breaks git (prove round 1 of #653). Evidence in `.prove-it/runs/` stays.

## Gotchas

- When head and base have the same package versions, the base build in the shared target
  directory replaces `meada-core` artifacts that Cargo would reuse for a head UI build (#662
  saw 22 missing-field/type errors). `build.sh` touches the head's local Rust sources before
  its build, so no manual `cargo clean -p meada-core` is needed. Keep the first failure logs
  if one still shows. Copied proof binaries remain immutable.
- CI uses the workflow's timeout and worker limits; `bounded` applies to local gate commands.
