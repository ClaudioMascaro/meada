---
name: review-meada-stack
description: Review a Meada PR stack or a builder's delivery. Check claims against cross-repository smoke evidence, verify issue drafts independently, and write the next builder prompt.
---

# Review a Meada PR stack

Start with the PR, builder transcript or scope the user supplied. For “all open PRs”, include
independent branches too. Return a verdict per tested revision, the findings that block it,
and a prompt another builder can use with a fresh checkout.

## Reconstruct the stack

From a checkout of Meada, list open PRs with both repositories, branch names and immutable tips.

~~~sh
gh api --paginate 'repos/ClaudioMascaro/meada/pulls?state=open&per_page=100' \
  --jq '.[] | [.number, .base.repo.full_name, .base.ref, .base.sha,
               .head.repo.full_name, .head.ref, .head.sha] | @tsv'
~~~

Match a child's target repository and branch to its parent's source repository and branch.
Record each PR's actual merge-base and head after fetching the revisions. Compare that diff
to its issues and acceptance criteria. Review scope, correctness, regressions, test quality
and uncovered branches. Identify which PR introduced a behavior, which fixes it and which
only changes the driver. A descendant's evidence belongs to the revisions it actually ran.

## Check the builder's claims

For each material claim record the expected answer and its source, the input and binary
revisions, the observation, and a public evidence link. Follow the
[Proof protocol](../../../.claude/skills/prove-meada/SKILL.md) for scope, answer keys and
verdicts. Read the driver as it stood at the tested SHA. Match the workflow, attempt, artifact
and receipt to the claimed base and head. Verify the archive digest when published. Inspect
selected steps, raw exports, stderr and per-site checks. A skipped job, missing artifact or
stale SHA leaves the affected claim unproven. Account separately for expected historical
deltas, wrong answers and failures to run. Keep the workflow's capture status alongside the
semantic verdict. Record a maintainer decision when a golden or a change of contract needs it.

When the PR changes a smoke driver or its selection rules, run `scripts/prove-ci --rows`
on each production path that supplies the behavior (one path per invocation) and on an
unrelated path. Compare the selected flows with the Scope table and the reader the smoke
exercises. A full PR that also edits the driver can select every flow and hide an omission.

## Exercise the cross-repository smokes

Use the pinned `meada-smoke-*` repositories as **source inputs**. Keep their public origins
and sibling checkout names so Meada can discover peers and build one index per repository.
Take pins and commands from the receipt, mock PRs or the driver under review. An older
revision may predate that driver. Record the coverage gap and identify the driver revision
used for any later replay.

Use the repository's labelled `e2e` Proof workflow for product execution. When asked to run
it, check the PR's eligibility and existing runs, follow the run through completion, and
inspect the artifact for the exact SHA. Respect the draft and CI requirements in
[AGENTS.md](../../../AGENTS.md). Locally, prepare fixtures and inspect archived outputs.
If a needed case has no hosted capture, specify the source input, answer key and CI flow
needed to run it. Add mock sources to the existing corpus and assertions to Meada's driver
when implementation is in scope. Propose a new sibling repo only when the case needs a
distinct repository identity or topology the current peers cannot represent.

Choose stress cases that distinguish the claim from a plausible wrong answer:

- For peer resolution, use one caller with declared authority and one with two plausible
  providers. Reorder peers, move the authority and remove then restore a route. Check the
  exact caller, method/path, provenance, provider, level and reason. Join open items by
  call ID and compare kinds and candidates. Report both the numerator and denominator for
  each binary. Name genuine ambiguities preserved as well as wrong links removed.
- For persisted state, compare cold, warm and first export after swapping the binary without
  editing sources. For interrupted or concurrent operations, compare with a clean run and
  inspect on-disk state. Run the relevant CLI and MCP paths the issue promises.
- Challenge the scorer with a copy of a raw response. Try a missing consumer with a surviving
  link, a same-named neighbor or contradictory candidates. Record such experiments as tests
  of the driver. Check that a repaired head can pass with a known-bad base and that a
  regression still fails. Keep the original outputs for comparison.

## Verify findings before publishing

Search open **and closed** [tracker issues](../../../docs/agents/issue-tracker.md), PRs and
prior review comments by symptom, source site and expected behavior. Reuse a ticket when its
acceptance criteria cover the observation. Otherwise draft an issue with exact revisions,
minimal reproduction, expected and observed results, a control and acceptance criteria.
Distinguish a product bug, a faulty oracle and an unresolved maintainer decision.

Do the review inline. At the evidence milestone give a fresh subagent the revisions, issue
criteria, fixture pins and raw evidence. Ask it to reproduce or refute the claims independently
before showing it the proposed findings. Then have it check the drafts and possible duplicates.
Correct discrepancies before publication. If independent review is unavailable, identify which
claims still need it. When publication was requested, check the tracker again, publish verified
new issues and add the new evidence to matching tickets. Comment on the affected PRs with
their own tested SHAs. On a repeat review, update an existing comment or add just the new
evidence. Return the public links. For a draft-only request, return the texts to the user.

## Hand off to the builder

Write a self-contained prompt in the user's language. Include the following:

1. Target repository, PR and branch for each fix, pinned revisions, and the public
   issue, run and artifact links. Explain which dependencies require stacking. For a
   separable fix, try its patch on current `main` in a disposable worktree, run the relevant
   checks and report exactly what that direct PR would unblock.
2. Each blocking issue with its expected behavior, observed failure and passing control.
   State which decisions belong to the maintainer and which work can proceed meanwhile.
3. The mock origins and pins, reproduction commands, proof driver revision, required
   regression and stress cases, and how to compare the candidate with the original failing
   revision. Give a measurable completion criterion per issue.
4. The gates and exact-SHA evidence the builder must return. Identify any remaining gap or
   unapproved golden. The next session must be able to start from the public references.
