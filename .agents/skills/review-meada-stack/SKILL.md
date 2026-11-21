---
name: review-meada-stack
description: Review a Meada PR stack, all open PRs, or a builder delivery against its issues and Proof evidence. Prepare the next builder handoff.
---

# Review a Meada PR stack

## 1. Map the requested PRs

Start with the PRs and claims in the request or builder transcript. Look up any cited PR
that has since closed. List open PRs with their source and target repositories, branches and tips:

~~~sh
gh api --paginate 'repos/ClaudioMascaro/meada/pulls?state=open&per_page=100' \
  --jq '.[] | [.number, .base.repo.full_name, .base.ref, .base.sha,
               .head.repo.full_name, .head.ref, .head.sha] | @tsv'
~~~

Match each child's target repository and branch to its parent's source. Fetch the revisions
and compute each merge-base. For a stack review, follow its dependencies. For all open PRs,
include independent branches. Account for each requested PR with its merge-base, head,
scope and issue criteria. Identify which revision introduces the behavior under review.

## 2. Judge each claim on its tested SHA

Review the diff for correctness, regressions, test quality and uncovered branches. Use
the [Proof protocol](../../../.claude/skills/prove-meada/SKILL.md) for answer keys and verdicts.
Match the run, attempt, receipt, driver and binary revisions. Check the published artifact
digest when available. Read the raw exports, stderr and per-site checks for the claimed step.

For cross-repo indexing, persistent state or changes to a smoke driver or its selector,
use the [smoke checks](../../../.agents/skills/review-meada-stack/smokes.md). When asked to
run product E2E, check for an eligible run on the exact SHA. Reuse it or trigger the
labelled workflow under [AGENTS.md](../../../AGENTS.md), then follow it to completion.
Locally, prepare inputs and inspect captured outputs.

For each material claim, record the expected result and its source, what the tested revision
did, the evidence link and the semantic verdict. Report the capture status separately.
Distinguish a historical delta, a wrong product answer, a faulty oracle and a failure to run.
For missing, skipped or stale evidence, name the exact input or run still owed. Attribute a
descendant's recovery to its SHA and keep the parent's result visible.

## 3. Verify findings and drafts

Do the review inline. Search open and closed [issues](../../../docs/agents/issue-tracker.md),
PRs and earlier comments by symptom, site and expected behavior. Match each finding to
an existing ticket's criteria, a maintainer decision, or a new draft. In each draft give
the SHA, minimal reproduction, expected and observed outputs, a passing control and
acceptance criteria.

Give a fresh verifier subagent the revisions, criteria, fixture pins and raw evidence first. Ask it
to reproduce or refute the claims. Then give it the drafts and proposed ticket matches to
check for duplicates. Reconcile disagreements against the evidence. Mark each finding as
independently verified, refuted or still pending, including when a verifier is unavailable.

## 4. Return or publish findings

For a draft-only review, return the drafts. When the user requests publication, recheck the
tracker, add evidence to matching tickets and publish verified new issues. Comment on each
affected PR with its own tested SHA. On repeat reviews, update the existing comment or
post the new observation. Give the user the links and identify anything still pending.

## 5. Write the builder prompt

For each fix, name the owning PR and target branch. If the patch could land directly on
`main`, try it in a disposable worktree at current `main`, run the relevant checks and
report what that PR would unblock. For a stacked fix, name the dependency.

Write the prompt in the user's language. A builder with a fresh checkout must be able to
locate the failing revision and reproduce each blocker. Give public issue, run and artifact
links, fixture origins and pins, driver and binary revisions and commands. For each blocker,
name the regression and stress cases to rerun on the failing SHA and the candidate, the
passing control and the result that establishes recovery. Specify the gates and exact-SHA
evidence to return, any golden or contract decision awaiting the maintainer, and the work
that can proceed.
