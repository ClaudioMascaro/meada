---
name: implement
description: "Implement a piece of work based on a spec or set of tickets."
disable-model-invocation: true
---

Implement the work described by the user in the spec or tickets.

If the user passes a ticket reference, fetch it from the issue tracker and state its title before starting. If the reference is ambiguous, ask.

Before editing, run each acceptance criterion's failing claim against the base. A criterion that already holds there is covered: report it with the evidence and do not re-implement it.

Call the Skill tool with "tdd" where possible, at pre-agreed seams.

Follow `AGENTS.md` for validation scope and the gate. Use focused checks while implementing (`scripts/check --scope`, or `--scope auto` for the changed modules), then run the applicable `scripts/check` gate before pushing; on a machine holding `~/.config/meada/ci-gate`, that gate is CI's and the script says so. Red and mutation runs call `cargo test` on the test's name (through `bounded` when it is on PATH): `scripts/check` checks the staged candidate, formatted, and refuses anything else. Documentation, instructions and harness-only edits get static and self-checks, with E2E `N/A` in the validation summary and no proof receipt. Prepare or read CI evidence only when the E2E policy applies.

While a build, a check or a subagent runs, start it in the background and act on its notification: no polling, no peeking. Meanwhile read the next piece of work or draft its test in the scratchpad. A build reads the tracked files, so they stay untouched until it ends.

Before committing, each new sentence of a ceiling, the CHANGELOG or a doc comment names the test that covers every form it states, or drops the form.

Once done, call the Skill tool with "code-review" to review the work.

Commit your work to the current branch.
