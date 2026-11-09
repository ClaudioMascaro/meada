---
name: maintain-prove-it
description: Maintain verification steps from prior receipts and reported misses while preserving independent answer keys and the AGENTS.md execution policy. Use when explicitly asked to update a stale verifier or investigate a verifier miss. Validate documentation and harness changes with static and self-checks; product E2E stays on its permitted CI path.
---

# Maintain Prove It

A verification skill goes stale the day the app changes: a route moves, a button gets renamed, a new surface ships with nothing in the skill that reaches it. A stale skill is worse than none, because the next agent will trust it. Every prove run leaves a receipt that shows where the skill struggled. This pass reads those receipts and folds what they learned back into the skill.

## Rules for the whole pass

- **Only edit the skill's own folder** (SKILL.md, scripts/). Never change product code here.
- **When the app and the skill disagree, work out which one is wrong.** The skill describing something the app no longer does is either *drift* (the app changed on purpose: fix the skill) or a *product bug* (the app broke: report it, don't rewrite the skill to match the bug). Read the commit that changed it. If you still can't tell, ask.
- **Answer keys are not the skill's steps.** The user's spec, the snapshots in `.prove-it/snapshots/` and the old system a flow is compared with say what right looks like. A drifted step can be fixed, but an answer key that disagrees with the app is reported to the user, never rewritten to match. Only the user changes one.
- **Validation follows `AGENTS.md`.** Skill and harness edits get pertinent static and self-checks locally, E2E `N/A` in the normal validation summary and no proof receipt. Product E2E runs only through the selected CI path, never because a skill changed.
- **Evidence retains its identity.** Earlier receipts and artifacts keep their tested SHA and scope. They are leads for new behavior, not proof of HEAD. Pending product evidence remains pending; `CAPTURED` is never `PROVED`.
- **Never loosen a check to make a flaky step pass.** Record the failing run, then decide whether the app has a race or the check is ambiguous.

## 1. Find the skill

Look for `prove-*` folders under `.claude/skills/` and `.agents/skills/`, at the repo root and in package folders. Prefer ones with the `prove-it:` marker comment. The marker says which kind it is: `permanent skill` (the app, one section per surface) or `temporary migration skill` (one migration, deleted at its end step). If there are several, ask which. If there are none, stop and suggest `/create-prove-it`.

## 2. Read the past runs

List the receipts in `.prove-it/runs/`, newest first. Start from the runs since the skill last changed (`git log -1 --format=%cI -- <skill folder>` gives the date). If the skill is uncommitted or there are only a few runs, read them all. For each receipt, note:

- **Failed or blocked steps**, and what the receipt says caused them.
- **Retries and fixes**: the command, port, selector or auth step that finally worked after the written one didn't.
- **Steps that failed for reasons unrelated to the change.** These are the clearest sign of drift: the change was elsewhere, but the skill's own step broke.
- **Not covered** lines that keep coming back, and the unlock each one names.
- **Scope**: the surfaces each diff reached. A changed path the Scope map doesn't list, or a surface the run drove with no section in the skill, means the map is behind.
- **What the change touches**: each flow the run listed as new or existing, its answer key, and any smoke test. A run that checked the whole app, skipped an existing flow behind shared code the change touched, or checked a new flow on parity alone when the change edited both sides, means the Sort checklist or the Scope map's shared-code rows need fixing.
- **Answer keys used**: new flows marked `no answer key` again and again means specs aren't reaching the skill. Say so, and suggest where the user could put them (the PR body, a file in the diff).

If there are no receipts, say so. Then use reported bugs (step 4), source history and static/self-checks. Report absent product evidence as pending.

## 3. For a temporary skill, check whether the migration is done

Look at the evidence, not the plan. When every area in the Scope table is moved and the last run is green, suggest the end step now, before the PR that deletes the old system: the snapshots must be recorded while the old system still runs. Also suggest it if the old system's code is removed or about to be, or all traffic is on the new system. The end step is: save the cases as snapshots, add them to `prove-<app>`, prove them there, then delete the temporary skill. Don't run it without their OK. If the old system is already gone and the end step never ran, say so plainly: the cases can no longer be recorded from it, so the snapshots must come from the last receipts that ran it, or the new system as it is now, marked as the weaker key.

If it isn't done, update the Scope table's Moved column from the commits since the skill last changed.

## 4. Collect bugs that slipped past

If the user hasn't already said, ask in your first message, before the other work, whether anyone reported a bug that prove should have caught. For each one, find the receipt of the run that said PROVED on that change, and work out why it passed: the flow wasn't in scope, the step didn't look at the right thing, or the seed data hid it.

## 5. Sort what you found

Turn the notes into a list of edits, each tied to the receipt or bug that shows it is needed:

- **Drifted step**: the app changed on purpose and the step didn't. Fix the step.
- **Working command**: a retry found what works (a port, a wait, an auth step, a flag). Write it into the skill now, while the receipt still says exactly what it was.
- **Slipped bug**: add the check that would have caught it, and name that kind of bug in the surface's "Could fool you" notes along with what now catches it. If the flow had no answer key (rung 5), propose one: a parity check against an old system, a snapshot from a trusted version, or spec lines the user approves.
- **New surface or path**: recent diffs added a route, page, command, job or folder the Scope map doesn't reach. Add it to the map. A new path on a known surface goes into that surface's section. A whole new surface gets its own section, with a drive recipe, what it copies (its parity answer key, if an existing surface does the same job) and "Done looks like" notes. Only call it new if you can name the file that adds it.
- **Shared code moved**: a schema, auth, nav or shared module was added, split or renamed. Update its row in the Scope map so the skill still finds the existing flows behind it.
- **Removed surface**: take its map row and recipe out, citing the commit that removed it.
- **Launch changes** (new env var, port, service, migration, dependency): update Start and Health.
- **Answer key disagrees with the app**: a spec line, a snapshot or the old system no longer matches what the app does. Don't touch it. Report it with the evidence, and the commit that changed the behaviour, so the user can decide whether the app or the answer key is wrong.
- **Product bug**: the app is really broken. Record it with the evidence. It is for the user, not this pass.

## 6. Edit and validate under the repository policy

Make the edits, then run pertinent static and harness self-checks. Re-run Scope on a recent
receipt's diff and confirm it names the intended surfaces and answer keys. Check that local
invocation prepares CI inputs or reads artifacts, and that docs-only edits do not activate E2E.
Report E2E `N/A` in the ordinary validation summary; do not create a proof receipt for this edit.

For a slipped-bug check, prepare the failing-version case for the selected CI runner and retain
the independent oracle. It counts as product proof only when CI evidence shows it fails on the
broken version and passes on the fixed version, with their original SHAs. Until then, say the
new check is statically validated and product evidence pending. Missing prerequisites are
`BLOCKED`; a product disagreement is `BROKEN`. Report product bugs without changing product code
or answer keys. Keep cleanup and evidence-retention steps in the CI recipe.

A manual mutation audit is a separate explicit `workflow_dispatch` request. Do not plant a bug
locally or start an audit merely to validate a skill text edit.

## 7. Report

- **Outcome:** `current` (nothing needed changing), `updated` (the skill changed and its pertinent static/self-checks passed; name pending product evidence) or `blocked` (say exactly what stopped you).
- What changed in the skill, one line each, with the receipt or bug that called for it.
- Product bugs found, with evidence. These are for the user to fix, not this pass.
- Answer keys that disagree with the app, and answer keys you propose, for the user to approve.
- For a temporary skill: whether the migration looks done, the evidence, and whether you suggested the end step.
- Surfaces left unproven and why.

Follow the repository's commit policy after the applicable gate passes.
