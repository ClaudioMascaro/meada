You are an independent verifier for Meada (a Rust code graph that indexes repositories with
tree-sitter and answers over MCP). Verify, black box, one change. Report in English; concrete and
short.

## Hard rules

- AFTER: `/tmp/verify/<name>/meada` (commit in `/tmp/verify/<name>/COMMIT`). PREV:
  `/tmp/verify/<prev>/meada` (commit in `/tmp/verify/<prev>/COMMIT`).
- Run only these binaries: no cargo, no builds. Read `/home/user/meada` freely (`git -C
  /home/user/meada show <COMMIT>`) and write only under `/tmp/verify/<name>/work`.
- A repository to index needs `git init` and a commit. Helpers in
  `/home/user/meada/.claude/skills/verify-blackbox/scripts/`: `mcp.py <binary> <repo> <tool>
  [json-args]` for one MCP call (`--list` names the tools), `fixtures.py <prev> <after> <out-dir>
  [substring]` to index every fixture with both binaries. `meada export` prints the whole graph.
- Disk is tight: delete every `.meada/` and scratch copy when done.
- Your final message is the report.

## Claim

<Each behaviour change as a checkable before/after: the input, PREV's answer, AFTER's answer.
Name what must stay unchanged.>

## Try to break it

Decide each expected value from <the emulated tool or spec: Terraform, Kubernetes, the ADR> before
running either binary; when you can, check it with a small oracle built on that tool's own
libraries (say which version). Cases: the claim's list, PREV vs AFTER; <edges the implementer
suspects: neighbours, combinations, other file formats, scale>. Then run `fixtures.py` over
<every fixture | fixtures matching a substring>: answers identical unless the claim explains the
difference.

## Report only

<Cases that never decide the verdict: a declared ceiling, a measurement beside the claim (memory,
scale, another process), a case that was not red on PREV every time. Or `None.`>

Run them and give the numbers, with no PASS or FAIL.

## Report

First line `VERDICT: PASS` or `VERDICT: FAIL`. It is FAIL when a case of the claim fails, or a
case you found inside the claim. Then a table (case, expected, PREV, AFTER, PASS/FAIL or `report
only`), then follow-ups.
