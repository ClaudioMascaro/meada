---
name: verify-blackbox
description: Independent black-box verification of a committed Meada change by a background subagent that runs the built binary against its own expected values. Use after a product commit when the user wants independent verification, or a fix's claim needs a reader not anchored on the implementation.
---

The verifier is a fresh subagent: it sees the claim and two binaries (PREV and AFTER), never your
reasoning, and decides expected values from the emulated tool's own semantics before running
either binary. Its report is development evidence; E2E evidence stays with `prove-meada` in CI.

## Steps

1. **PREV.** Reuse the AFTER of the last verification (`/tmp/verify/<prev>/meada`). For a first
   run, stage before your first edit (step 2 on the base commit). Done when a PREV binary built
   from the commit before the change exists.

2. **Stage AFTER.** Commit, `cargo build --locked`, then
   `.claude/skills/verify-blackbox/scripts/stage.py <name>` (a short name for the change). It
   refuses a stale binary or uncommitted product sources. Done when it prints the staged size.
   A pushed commit needs no build where its `Diff` run kept the binary: a green product run of
   the commit itself, on a branch whose `diff.yml` has the step, for seven days (`gh run
   download <run> -n meada-debug-<full sha> -D /tmp/verify/<name>`, `strip` and `chmod +x` it,
   write the sha to `COMMIT`, `mkdir work`). A commit with no artifact (the base on `main`, a
   commit pushed under a later one) is built here as above. Behaviour compares across a runner's
   build and a local one; a cost compares two binaries of one lane only.

3. **Brief.** Copy [`brief.md`](brief.md) to `/tmp/verify/<name>-brief.md` and fill every `<...>`.
   The claim states each behaviour change as a checkable before/after, with the cases you
   already tested, so the verifier hunts past them. Each behaviour change is a case you saw
   _red_ on PREV: PREV gave the answer the claim says it gives (a hang or a kill counts), in
   every run you made (three for a case that depends on timing). A case of the claim that
   measures AFTER against PREV's number (time, memory) has that number: PREV ran it to its end.
   Under `## Report only` go a declared ceiling, a measurement beside the claim (memory, scale,
   another process), and a behaviour change or a measurement that fails its test. What must
   stay unchanged stays in the claim. A time is no case of the brief unless the claim is one:
   it needs a quiet machine, and the verifier shares this one with your builds (step 6). Done
   when no placeholder remains, every claim names its PREV and AFTER values, and each behaviour
   change was red on PREV.

4. **Launch.** Agent, `general-purpose`, background, prompt
   `Read /tmp/verify/<name>-brief.md and follow it exactly.` Keep working meanwhile.

5. **Hand-back.** First write the report verbatim to `/tmp/verify/<name>/REPORT.md`; the
   transcript is not a durable copy. Then, for each FAIL: reproduce it, fix it test-first, or
   record it as a ceiling in `docs/agents/ceilings.md`'s topic when the fix is out of reach. A
   `report only` row where AFTER answers wrong gets a ceiling or a follow-up issue. Done when
   every FAIL has a commit or a ceiling, every such row a ceiling or an issue, and the PREV
   directory is deleted (disk is tight; this AFTER becomes the next PREV).

6. **Cost, when the change adds work.** Optional, yours, after the verdict: with nothing else
   building, index one generated repository cold with PREV and AFTER, alternating, at least
   twice each, and report the commit, the build, the case and the numbers (`CODING_STANDARDS.md`,
   "Claims the diff adds"). Done when the PR body has them, or says no time was measured.

## Scripts

`scripts/check` runs the `--selftest` of each one a change touches.

- `scripts/stage.py <name>`: `/tmp/verify/<name>/{meada,COMMIT,work/}`, binary stripped; it takes
  the newest build of this tree from its own `target/`, `CARGO_TARGET_DIR`, or the main
  checkout's `target/` when `bounded` recorded this tree as its last builder there.
- `scripts/fixtures.py <prev> <after> <out> [substring]`: indexes each `tests/fixtures/` entry
  with both binaries, prints `ok` or the difference (export minus `commit`, `status`, exit).
- `scripts/mcp.py <binary> <repo> <tool|--list> [json-args]`: one MCP call over stdio.
