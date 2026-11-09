---
name: create-prove-it
description: Create or extend an app or migration verification skill with independent answer keys, scoped checks and evidence review that follow the repository's AGENTS.md execution policy. Use when explicitly asked to set up verification or add a verification surface.
---

# Create Prove It

Create a verification skill whose activation and execution follow the repository's `AGENTS.md` policy, with independent answer keys and explicit evidence limits.

The output is a project-local skill, read cold by a future agent in the middle of some other task. Write it for that reader: exact commands, real selectors, no placeholders, nothing it has to work out.

The steps, in order:

0. Ask: the app, or a migration?
1. Map the surfaces
2. Check you can reach them
3. Find the answer keys
4. Brainstorm methods, then let the user pick
5. Generate the skill
6. Validate through the permitted execution path
7. Hand over

Read `AGENTS.md` before step 1. It owns validation scope and where product execution is allowed.
In this repository, product E2E runs only in GitHub Actions for selected large implementations:
locally prepare inputs or review artifacts. Skill, documentation and harness-only edits get static
and self-checks, E2E `N/A` in the normal validation summary and no proof receipt. Keep existing
pending evidence pending. Manual mutation audits require explicit `workflow_dispatch`; never
schedule one because this generator ran or its output changed.

Every check has three parts: a **surface** (where a real user or program meets the change), an **answer key** (what right looks like, written by someone other than the agent that made the change) and a **comparison** (does what happened match the answer key).

## What the change touches

The generated skill checks what the change touches: every flow the change touches, each against the answer key that fits it. It works out those flows from the diff, and each is one of two kinds:

- **A new flow**: the change adds it. If it copies something that exists (an MCP server that must behave like the web app), the answer key is parity with that thing: the same action through both, then compare the data. Otherwise the answer key is the user's spec. If the same change edits both the new flow and the thing it copies, use the spec AND parity, because both can be wrong the same way and parity still passes.
- **An existing flow** the change touched, directly or through shared code (schema, auth, nav, a shared component or function). The answer key is the app before the change: a snapshot, or the old version run beside the new one.

A selected product E2E change made only of new files gets a smoke test of the existing app through the permitted execution path. Documentation or harness files do not activate product checks.

Never "check the whole app". The diff decides what gets driven.

**You never write the feature spec.** Specs come from the user's normal planning. The generated skill finds the spec after the build (the PR or issue body, a spec file in the diff, a path the user passes). If there is none, it asks. If the user has none, that new flow goes on the receipt as "no answer key", the weakest rung.

## 0. The app, or a migration?

Ask this first, before anything else: "Is this for the app, or for a migration?" Give the rule with the question: **if the old thing is deleted at the end, it is a migration. If the old thing stays, it is the app.**

- **The app** gets one permanent skill, `prove-<app>`, with one section per surface (web, API, MCP, CLI). If one already exists (a `prove-*` folder with the `prove-it:` marker), you extend it. "I'm adding an MCP server" is the app: add an MCP section to the existing skill, never a second skill. A new surface that copies another (the MCP server copies the app) names that surface as its parity answer key.
- **A migration** (Python API to Rust, Next.js to React Router, Zig to Rust) gets a temporary skill, `prove-<migration>`, such as `prove-rust-migration`. Its answer key is the old system: run old and new side by side on the same inputs and compare. Everything the migration covers is what the change touches, and the old system is its answer key. It ends with an end step: move its cases into `prove-<app>` as snapshots, then delete itself.

A Kitty image protocol added to Ghostty is the app, not a migration: Kitty stays, so the parity check can stay too.

If the request already answers it ("I'm adding an MCP server" to an app that stays), state your pick in one line and continue.

If nobody can answer (a headless run), infer it from the repo and the request, and say in the report which you picked and why.

## 1. Map the surfaces

A surface is where a user, human or program, meets the app. That is the only place a proof counts. Read the repo (README, package scripts, Makefile, routes, CLI entry points, Dockerfile, CI config, existing e2e specs) and list every surface you find:

| Surface | Typical sign in the repo |
|---|---|
| Web UI | pages/routes, a dev server script, HTML/JSX/Svelte/Vue files |
| HTTP API | route handlers, OpenAPI file, `fetch` clients |
| MCP server | tool definitions, an MCP SDK import, a `/mcp` route |
| CLI / TUI | a `bin` entry, argparse/commander/clap/cobra, a `main` that reads argv |
| Database / stored state | migrations, ORM models, a JSON/SQLite file, a bucket |
| Background work | queues, cron, workers, webhooks, email sending |
| Desktop / mobile | Electron, Tauri, Xcode or Android projects |
| Library | a package export other code imports |
| Agent / prompt | prompts, tool definitions, an agent loop |

Name a primary surface (the one most users touch) and list the rest. When you are adding a surface to an existing skill, map the new one and the surfaces it shares code with. If the new surface isn't built yet, mark it planned.

For a migration, map what it covers: every route, command or output the old system serves that the new one must serve too, and which parts have already moved.

## 2. Check you can reach them

For each surface, inspect the permitted runner, commands and required services. Under CI-only
policy, prepare the launch, health and comparison recipes for GitHub Actions and read existing
artifacts with their original SHAs. Do not start either product locally. Missing CI evidence is
pending, not a failed local reachability test. For a migration, keep old/new inputs and ports
isolated in the CI recipe.

When a surface is out of reach, don't quietly drop it. Suggest the unlock that would make it reachable. See [references/unreachable-surfaces.md](references/unreachable-surfaces.md) for the common blockers and their fixes. The user decides whether the unlock is worth it; your job is to make sure they know it exists.

**Presence is not access.** For every key, env var or account the verifier will need, validate access through the permitted runner (a whoami, list or login) and note its evidence and SHA; without that run, mark access pending. A variable that is set but empty, a secret that reads back blank or an expired token should surface here, not as a 401 forty minutes into a run. If the app behaves differently by role or plan, check one account per role. Then tell the user, in one message, exactly what is missing and what they would need to supply or sign in to. Stop before anything that takes their card, password or 2FA.

## 3. Find the answer keys

A check with no answer key only proves the agent agrees with itself. Answer keys sit on a ladder:

| Rung | Answer key | Example |
|---|---|---|
| 1 | An old system that already does the same job | The Python API a Rust rewrite replaces; the web app a new MCP server must match; the base commit run beside the change |
| 2 | A snapshot: a saved copy of a result you know is good | A screenshot from main, saved API responses, CLI output, generated files, in `.prove-it/snapshots/` |
| 3 | An independent test suite that does not depend on the code it checks | A behaviour suite that drives the public surface, like Bun's TypeScript tests when it was ported from Zig to Rust |
| 4 | A spec: flows and acceptance criteria written before the code | The user's spec, found in the PR, the issue or the diff at run time |
| 5 | Nothing: the checking agent decides what right means | The weakest. Flag it |

Name them per surface and per flow kind:

- **New flows, per surface.** Does this surface copy another? Then its answer key is parity with that one (rung 1). For an MCP server that copies the app: reading a record through both gives the same fields (read parity), bad input gets the same refusal (error parity), and create, edit and delete through MCP leave the same stored state as doing it in the app (lifecycle round trip). Otherwise a new flow uses the spec the skill finds at run time (rung 4).
- **Existing flows, per surface.** The app before the change. Say how: snapshots already in `.prove-it/snapshots/`, snapshots you take now from main for the main flows, or the base commit run in a worktree under `/tmp` on a second port.
- **For a migration**, the old system is the answer key (rung 1), plus an independent test suite if one exists (rung 3). Agree with the user on the inputs (recorded traffic, a fixture set, the old system's own test inputs) and on the list of differences that are allowed (ids, timestamps, a bug the new system fixes on purpose). Anything off that list is BROKEN.
- **Budgets are answer keys too.** A number the user sets (a layout shift score, a load time, an accessibility score, zero console errors) gives a clear pass or fail with no judgment. Name one per surface where it fits.
- **Say who wrote it.** A human, the old system, a snapshot taken from a version the user trusts, or a spec agreed before the code. Never the agent that will make future changes: it would grade its own work.
- **Never edit an answer key to make a check pass.** Only the user changes one.
- **Flag every flow whose only answer key is "nothing"** and tell the user. Offer the cheapest way up the ladder, usually a snapshot taken now from main.

## 4. Brainstorm, then let the user pick

This is the step that makes the skill fit the repo instead of being a generic browser script.

Read [references/methods.md](references/methods.md) for the catalogue of verification methods. Keep only the ones that apply to the surfaces you found, and present them as one table:

| # | Method | Surface | Answer key | Catches | Misses | Cost (setup / per run) | Reachable now? |
|---|---|---|---|---|---|---|---|

Rules for the table:

- **Catches and Misses are about this repo**, not the method in general. "Catches: the open count in the header not updating after a tick" beats "catches UI bugs".
- **Cost is honest.** Setup cost (install a browser, seed an account) and per-run cost (seconds, tokens, flakiness) are separate numbers.
- **Answer key names the rung** ("1: same call through the REST API", "4: spec", "5: nothing"). A method with no answer key can still catch crashes and swallowed errors, but say so.
- **Unreachable rows stay in**, with the unlock in the Reachable column ("No: needs Stripe test keys, or run `stripe listen` locally").

Under the table, recommend a combination in two or three sentences: usually the primary surface driven end to end, plus one cheap check that covers what the primary misses (a stored-state check under a UI flow, a log scan for swallowed errors). For a migration, recommend how to compare old and new: shadow traffic, byte-for-byte output, or stored state after the same inputs.

Offer measurable budgets in the table where they fit (see the method in methods.md). For a web UI, always offer these 4 as separate rows, each with a proposed number: layout shift (CLS), load time, accessibility score and console errors. Then go past the catalogue: ask what the user cares about for this app (speed, accessibility, SEO, design system rules, cost per request, a flow that must never break) and propose a measurable check for each goal, even one that methods.md doesn't list. The best checks often come from the user's goals, not from the code.

Then ask the user to pick, as a multi-select question with the recommended combo first. Use your agent's question tool if it has one (AskUserQuestion in Claude Code), otherwise ask in chat and wait. If nobody can answer, go with the recommendation and say clearly in the final report that you chose for them.

## 5. Generate the skill

**Where it goes.** Write it into the project skills folder of the agent you are running in: `.claude/skills/<name>/` for Claude Code, `.agents/skills/<name>/` for Codex. If the repo already uses one of those folders, use that one. In a monorepo where only one package has the app, put it in that package's skills folder. Name the permanent skill `prove-` plus a short app name (`prove-shop`), and the temporary one `prove-` plus the migration (`prove-rust-migration`).

**Extending, not replacing.** If `prove-<app>` exists and you are adding a surface, add its section, its rows in the Scope map and its answer keys. Leave the other sections alone, and tell the user what you added.

**What goes in it.** Follow [references/skill-template.md](references/skill-template.md): the permanent template for the app, the temporary one for a migration. Every section must be filled from what you actually saw in steps 1 to 4. The parts that matter most:

- **Scope**: how to work out the diff (the base branch, the exact git commands) and a map from the repo's folders to the surfaces and flows they reach. Mark shared code (schema, auth, nav, shared components and functions) and what it reaches, since that decides which existing flows the change touched. A migration's Scope is everything it covers, not the diff.
- **Description**: activate on explicit preparation or evidence-review requests and on the E2E selection policy in `AGENTS.md`. Source edits, task completion and bug reproduction alone do not activate E2E. State whether local use only prepares inputs or reads CI evidence.
- **Sort the change** (permanent only): the first step after Scope. A short checklist that sorts each flow the change touches (new or existing, copies something or not, did the change also edit the thing it copies) and picks its answer key. It is the flow chart in the README, as text.
- **Find the spec** (permanent only): where to look for a new flow's spec, and what to do when there is none.
- **One section per surface**: how to drive it, what it copies (its parity answer key, or "nothing"), what done looks like there, and how a check there could fool you.
- **Check in a fresh agent**: the agent that made the change works out the scope and sorts it, then, if it can start a sub-agent, hands the evidence review to a fresh one with only the flows, surfaces, answer keys, CI artifact and tested SHA. Not its plan or its reasoning: a checker that knows what was meant sees what it expects. If it can't start one, it reviews evidence itself and marks the judgment "not independent", preserving any requirement for an independent judge as pending.
- **If it is BROKEN**: the findings (failing steps, the answer key each was compared with, the evidence) go back to the agent that made the change. It fixes the app, never the answer key or the check, and the check runs again on the same flows. Cap it at 3 rounds, then report what is still broken.
- **Start**: a recipe for the permitted runner with the exact command, an isolated port and data location, and how to tell it is *usable*, not just listening. The start script must fully detach the app (redirect its stdin, stdout and stderr) so a caller doing `RUN=$(start.sh)` gets control back. A start script that hangs its caller is the most common first bug.
- **Health check**: one read-only command that answers "is this instance worth driving?" It also prints identity: which target (local, staging or prod, host, database) and which build (commit or version), so a check never runs against the wrong copy or a stale build. Don't trust "no rows found" until the store has shown a row you know exists.
- **Bug fix recipe**: run the probe on the unchanged code first to see the failure and record the value that shows it (status, row count, text), then after the fix to see that value change. If the failure doesn't reproduce first, say so and stop.
- **Deploy check** (only if the user ships by deploying): wait until the new build is actually serving, then drive the public entry point with no insider shortcuts, and label each receipt line with the environment it came from.
- **Try to break it**: the probes worth running on this app (empty input, doing it twice, stale state, wrong method, bad id), plus a cold start (empty data, no optional secrets) and one sibling flow that should behave the same way.
- **Evidence, receipt, clean up and gotchas**: as in the template.
- **End step** (temporary only): when the old system is about to be deleted, save each case's old-system output as a snapshot, add those to `prove-<app>` as answer keys for existing flows, prove them green there, then delete the temporary skill with the user's OK.

**Make the checks hard to fool.** When Prove It was tested, `/audit-prove-it` planted 7 bugs in a sample app. The agents doing the checking caught 5, but the generated scripts caught only 1. A second test on a real app got the same result: the scripts caught 1 of 6. The rules below close the gaps it found:

- **Seed more than one record**, some in each state (two done, two open). With one row, "counts only the first", "ticks every row" and an off-by-one counter all pass.
- **Cover every branch of shared code.** For each shared function in the Scope map, seed and script every branch it takes: each searchable field, each sort order, each filter value. Run the scripted checks on the base commit and on the change, and diff the results.
- **Check every action in reverse too**: untick, undo, remove the thing you added.
- **Let stand-ins fail.** A fake email or payment provider that always says yes can't show you the error path. Give it a switch that makes it fail, and check the app reports the failure instead of claiming success.
- **Assert everything the answer key and "Done looks like" name for that surface**, not just the field the change is about.
- **A script pass is not the verdict.** Scripted checks report "checks passed" or `CAPTURED`; only the judge, having compared the evidence to independent answer keys, writes PROVED.

**Scripts.** Put repeatable mechanics in `scripts/` (start, health, stop, and a scripted check per method where possible; for a migration, a compare script that runs one case through old and new and diffs the results). Make each one executable and show its exact invocation in SKILL.md. A scripted check is worth a lot here: `/audit-prove-it` can run it blind, and the next agent can rerun it in seconds. Select elements by stable attributes (a form field `name`, `data-*`, role plus route), not by visible text, unless the text is what is under test. Renaming one button must not break a script.

**Length.** Keep SKILL.md under about 250 lines, so a checker can read it in one pass. Put the detail of each surface in `references/<surface>.md` and keep a short section per surface in SKILL.md that links to it. Never make room by shortening a core block: the template lists them, and you copy them word for word.

**Evidence location.** Runs save to `.prove-it/runs/<UTC timestamp>-<slug>/` at the repo root. Add `.prove-it/runs/` and `.prove-it/audits/` to `.gitignore`. Snapshots are answer keys, so they live in `.prove-it/snapshots/` and get committed. Write evidence as it is produced, not at the end, so an interrupted run keeps what it proved. The receipt records what was proved, how the change was sorted, who checked it, the surfaces driven, each step's verdict, evidence and the answer key it was compared with, failed and retried steps, and the commands that finally worked. `/maintain-prove-it` reads these receipts later to improve the skill, so don't trim that detail.

## 6. Validate through the permitted execution path

For a skill or harness edit, run pertinent static checks and harness self-checks locally. Check
Scope and Sort on a real diff and confirm the generated activation follows `AGENTS.md`.
Record E2E `N/A` in the validation summary, without a `.prove-it` receipt.

When a product E2E is selected, prepare the generated recipes for the GitHub Actions runner:
start, health, one named flow with every selected method, a break-it probe, captures and cleanup.
Review that CI artifact against independent answer keys; until it exists, say validation is
pending. Fix broken recipe mechanics without weakening the answer keys. Verify cleanup and
retained evidence from the artifact. A migration may be BROKEN while the new system is unfinished.
Product E2E and manual mutation audits are separate: no mutation run is implied here.

Then check the core blocks survived. Each of these must be in the generated SKILL.md; put back any that is missing, word for word from the template:

```bash
for rule in "is not a spec" "UNPROVEN" "PROVED with a flag" "not changed" "What the change touches" "Report to the user" "Couldn't prove" "Check in a fresh agent" "If it is BROKEN"; do
  grep -qF "$rule" <skill folder>/SKILL.md || echo "missing: $rule"
done
```

A migration skill has no Find the spec or Sort the change, so it skips "is not a spec", "not changed" and "What the change touches".

Check activation statically: a docs-only edit, a source edit or a bug report by itself must not
start product execution. A selected E2E prepares CI inputs or reads evidence. If a planned surface
has no capture, list it as pending; do not produce a receipt that implies it ran.

If you had to prove it in a stand-in (a container instead of the user's real setup, a local stack instead of staging), list every step the stand-in did for you that the real target will not.

## 7. Hand over

Start with the actual outcome: static/self-check validation with E2E `N/A`, preparation with evidence pending, or the reviewed CI verdict (PROVED, BROKEN, BLOCKED or UNPROVEN), with its tested SHA. Then report, briefly:

- which kind of skill you made or extended, and why,
- the surfaces found and which ones are unreachable, with the unlock for each,
- the answer keys per surface and check, and every flow left on "nothing",
- the methods the user picked (or that you picked for them, and why),
- where the skill lives and how to invoke it,
- the CI run/artifact and reviewed receipt when available, with its original tested SHA and key evidence; otherwise the static/self-check results and what is pending,
- what is next under `AGENTS.md`: prepare or review selected CI E2E, maintain drift when requested, and run mutation audits only by explicit manual workflow dispatch. For a migration, propose the end step before deleting the old system.
