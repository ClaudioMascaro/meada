---
name: implement
description: "Implement a piece of work based on a spec or set of tickets."
disable-model-invocation: true
---

Implement the work described by the user in the spec or tickets.

If the user passes a ticket reference, fetch it from the issue tracker and state its title before starting. If the reference is ambiguous, ask.

Call the Skill tool with "tdd" where possible, at pre-agreed seams.

Follow `AGENTS.md` for validation scope and the gate. Use focused checks while implementing (`scripts/check --scope`), then run the applicable `scripts/check` gate before pushing. Documentation, instructions and harness-only edits get static and self-checks, with E2E `N/A` in the validation summary and no proof receipt. Prepare or read CI evidence only when the E2E policy applies.

Once done, call the Skill tool with "code-review" to review the work.

Commit your work to the current branch.
