---
name: meada-feedback
description: Feedback on a session's use of the meada MCP server, turned into proposals for its search, context and trace mechanisms.
disable-model-invocation: true
---

A **retrospective** of one session from the side of the code graph: what the agent asked Meada,
what it got, and what it looked up with grep instead. The product of this skill is a ranked list
of improvements to Meada's search mechanisms, each backed by the session's own queries.

## Steps

1. **Usage.** Run `.claude/skills/meada-feedback/scripts/usage.py` (the newest session of this
   repository; pass a `.jsonl` path for another, `--subagents` to add the session's subagents).
   Done when it prints the meada calls, the grep lookups across the tree and the totals.

2. **Meada calls.** Give each call one class, with one line of evidence from what the session
   did next: _answered_ (used, no search after it), _wrong_ (the code contradicts an `exact`
   answer, or a name match led astray), _empty_, _too large_, _missing capability_ (the answer was
   right but the agent still had to grep or read for the rest), _slow_. Done when every call has a
   class.

3. **Grep lookups.** Group them by what the agent wanted: a definition, its callers, a definition
   with its body (`-A` lines), a string or config text, a path outside the index (`tests/`,
   `scripts/`, a dependency). For each group, run the meada tool that should answer one
   representative lookup now and record what it gives. Done when every group has a
   representative, the tool tried and its answer.

4. **Proposals.** Rank improvements to `search`, `context` and `trace` (ranking, kinds, what an
   answer carries, coverage, size) by how many lookups each would have replaced times what the
   grep path cost. Each names the motivating queries. A group meada already answers, but the agent
   never asked, points at steering instead: the wording of `AGENTS.md`'s "Code graph" section.
   Done when every group from step 3 and every class except _answered_ from step 2 maps to a
   proposal or to "no change", with the reason.

5. **Report.** Present the proposals to the user in order, with a draft issue for each _wrong_
   answer in the form `AGENTS.md` asks (the query, the answer, the expected one). File issues or
   edit steering only when the user asks.
