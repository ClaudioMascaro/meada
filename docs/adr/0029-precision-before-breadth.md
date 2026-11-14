# 29. Precision before breadth

- Status: proposed
- Date: 2026-11-13
- On trial until the first yardstick report has run under its rules: acceptance is decided on
  that report. See "Open to revision".

## Context

"Certain or absent" (`CODING_STANDARDS.md`) binds whoever writes resolution: a wrong `exact` link
is the worst failure, and a missing one is an open item. It binds nothing else. A dogfooding pass
of 1.3.0 over 23 repositories found most `meada status` orphan lines false (ADR 0026, "Orphan
sides") and an `inferred` link to the wrong route (#801). Both were fixed by rule (#844, #845),
and nothing says how many wrong statements remain: no measurement of precision has run. Two are
planned, #795 for orphan lines and #835 for CI links, and the links of the other contract kinds
have none.

Server mode aims at a workspace the size of an organisation, with hundreds of repositories (ADR
0011). A link made by name collides more as a workspace grows, so a precision measured on 23
repositories does not carry over. The maintainer's time on the project is spent deciding, so a
rule that orders that time decides what ships.

## Decision

### Invariant 7, for ADR 0002

The text below enters ADR 0002 as invariant 7 when this record is accepted.

A wrong statement is worse than a missing one. A **statement** is a link or an orphan line.

- A wrong `exact` link is a bug.
- A statement of absence (an orphan line, a `no use` or `no provisioner` mark) is a statement and
  follows the same rule.
- What Meada knows it does not read shows as an open item or as a named ceiling. A silent absence
  a yardstick finds becomes a fix or a ceiling.
- Recall is not promised.

No number is part of the invariant. The numbers are thresholds of a yardstick, below.

### Yardsticks

Precision is measured per **pair**: a contract kind and a statement type (links, or orphan
lines). Links are counted per level; orphan lines have no level. A **yardstick** is the
measurement of one pair, kept as a ticket:

- **Corpus.** Public code pinned by URL, commit and licence. It is chosen in the ticket before
  the implementation, by a rule written first and run by someone who does not implement; the
  search need not be reproducible, its pinned result is.
- **Answer key.** Never written by whoever built what it judges. Where a record of execution
  exists it is the answer key, captured once and pinned with the corpus, and it is read one way
  only: what it shows and Meada did not link is a missed call, and its silence proves no link
  wrong. Elsewhere a model outside indexing judges, and the maintainer samples its marks (about
  50 lines, the "correct" ones included) before the report counts.
- **Verifier.** It lists the statements from the source before it opens the export. Marking an
  orphan line real takes a search for a real use over the whole corpus.
- **Thresholds.** Written before the run, per level and per origin:

  | Count | Threshold |
  | --- | --- |
  | wrong `exact` link | 0 |
  | silent absence: the answer key shows it, and Meada states nothing | 0 |
  | `ambiguous` link whose candidates lack the right provider | 0 |
  | wrong `inferred` link | at most 1 in 10, valid from 30 judged |
  | open miss: only an open item or a ceiling names it | per cause, see below |

  Each cause of an open miss is an entry the maintainer approved, with its expected count. A
  cause off the list counts 0, and a count above the expected one is a finding. Where the
  statements can be counted from the source (the forms of a CI `uses`), the answer key writes
  the expected count per form before the run, and the report must match it. The thresholds of
  orphan lines are written in their yardstick, per cause of a false line (#795, and the A9
  yardstick for the CI kind).
- **Floor.** A level, an origin or the links across repositories with fewer than 30 judged
  statements is **unmeasured**, not approved. Unmeasured is read from the last approved report:
  it is whatever has no judged statement there, an old origin the corpus does not exercise
  included. Each report counts statements per origin.
- **Below a threshold.** The release waits, or the level stays out of the default output. The
  first use of the second exit needs its own decision.
- **Ceilings.** A new ceiling is the maintainer's decision, one per entry, never the automatic
  exit of a failed criterion.
- **Distractor.** Where a link rests on a name being the only one in the workspace, the
  yardstick runs twice over its corpus, without and with unrelated repositories chosen by the
  corpus rule. The report counts how many names of the distractor collide with names the corpus
  calls: with no collision the distractor is unmeasured, not passed. A link then has three
  outcomes: it does not change, it becomes `ambiguous` (expected, and counted), or it stays
  `inferred` and changes its target, which is a wrong link. An `exact` link that changes is a
  bug. The count of links that became `ambiguous` enters the ceilings as a measured value.
- **Approved.** The maintainer closed the yardstick's ticket with every count inside its
  threshold.

The corpus rule and the thresholds above are the defaults, decided here once: a yardstick's
ticket records only where it departs from them. A number here changes only by a dated comment of
the maintainer. "Open to revision" lists the ones chosen with no measurement behind them.

A yardstick closes with the origins the usage questions exercise. The other origins of its pair
stay unmeasured in the gate list and do not hold the stop rule. The yardsticks of HTTP and of
package take the origins in decreasing order of sides (providers, consumers and unresolved
consumers) until they hold 80% of the sides of the pair, and any origin a usage question
exercises whatever its place. The order is by sides, not by links: a link exists only where a
reader worked, so an order by links would leave last the origin that reads badly. The gate
list issue gives the order and the cut; the counts behind them stay with the maintainer.

### The release diff

Before a tag, the links and orphan lines of the candidate are compared with those of the
previous tag, in two halves: a job on the release pull request over the yardsticks' pinned
corpora, and a line of the maintainer's dogfooding pass (#809) over a private workspace,
published in aggregates only.

- A change no changelog entry or issue explains holds the tag.
- A link that vanished and a level that dropped are judged against the code: if the link was
  right it is a regression and holds the tag even when explained.
- The explanation is written per group of changes of one cause, by an agent that did not make
  the changes, and the maintainer samples it.
- A new link and a raised level enter the report of their pair.

### The gate

A pair enters the gate when either holds:

1. a report recorded in the tracker found a wrong statement of the pair;
2. a usage question the maintainer recorded in the tracker, in neutral terms, is answered by
   reading the pair.

One issue holds the gate list: the usage questions, each pair in the gate with its date, its
reason and the yardstick ticket that covers each origin, and each pair out of it marked
unmeasured, so that absence does not read as approval.

On 2026-11-13 the gate holds the links and the orphan lines of the CI kind (a reusable pipeline
or an action nothing calls, #827: the A9 yardstick judges them with the links), the links of
HTTP and of package (neither has run a yardstick), and the orphan lines of provisioning and of
image with the links that remove one (#795: both are judged in one pass, and the report gives
two numbers, the wrong links per level and how many of them hide an orphan line). The orphan
lines of provisioning are the `no use` and `no provisioner` lines, of resource contracts and of
the channel contracts IaC provisions. The package yardstick is born with a distractor: a fork or
a copy of a public package that kept its name, with consumers that install the public one,
judged against the consumer's lockfile. Channel links and the alert kind are out, unmeasured.

### The stop rule

- Every new contract kind is specified with its yardstick ticket, and its release waits for
  that yardstick's approval.
- The specification of a new contract kind may be written and grilled, but none of its tickets
  is `ready-for-agent` before the yardsticks of the gate are approved.
- It is grilled only while no yardstick decision waits for the maintainer, and at most one
  specification of a new kind is open at a time. A yardstick decision waits when a yardstick
  ticket of a pair of the gate is open with `ready-for-human`, or has an open pull request
  whatever its label, or when a pair of the gate has no yardstick ticket. A specification is
  open while its issue is.
- While a yardstick of the gate is open this equals "no specification of a new kind", on
  purpose. An exception exists only as a dated comment of the maintainer on the gate list issue.
- A **new origin** of a pair is a newly registered module (a language pack, a package ecosystem,
  a kind's module for one language) or a new row of a command or format table. While the pair's
  yardstick is not approved it follows the rule of a new kind. After that it does not close the
  gate again, but the release that carries it waits for the pair's report run with it, and the
  thresholds hold for the origin alone.
- Improving the resolution of an origin that exists is not a new origin. The release diff of
  links catches what it multiplies: a new link and a raised level enter the pair's report.

### Spec A9

Spec A9 (#820, ADR 0028) is outside the stop rule: none of its tickets loses `ready-for-agent`,
and none is edited while its chain runs. Two decisions of this record land by tickets that
follow the chain, and until they do ADR 0028 reads as written:

- An install by name alone in a CI `run` script (`uvx` and `pipx run` included), and the program
  link that rests on it, are `inferred` until the package yardstick approves the rule that one
  provider of a name in the workspace is the package installed. An install that names its
  origin (a path, a VCS URL) stays `exact`.
- Each slice of A9 ships when its yardstick approves it. That yardstick, slice by slice, is
  what holds the tag; the report #835 gives as written stays as a base record. A slice the
  corpus does not exercise past the floor is unmeasured and does not ship.

## Open to revision

Five numbers of this record were chosen with no measurement behind them. Each is listed with the
reason for its value and the evidence that would change it.

- **The floor of 30 judged statements.** With none wrong in 30, a true rate above 1 in 10 has
  under 1 chance in 20 of giving that report (with 10 judged, a clean report still allows 1 in
  4). That holds only if the statements fail one by one. Evidence that changes it: a report
  whose wrong statements come from one rule, one form or one repository, since the floor would
  then count causes or sources, not statements; a pair approved at the floor that a larger
  report of the same pair, or the release diff, later shows outside its threshold; a corpus
  rule that reaches 30 only by choosing code Meada already reads well.
- **At most 1 wrong `inferred` link in 10.** The maintainer's estimate of how often a link by
  name may be wrong before the level stops helping its reader. At the floor it promises less
  than it reads: 3 wrong in 30 pass, and that report still allows a true rate near 1 in 4.
  Evidence that changes it: the base report of #795, after which the maintainer confirms the
  number or takes 1 in 20 (which needs about 60 judged); first reports that all sit far under
  it; an answer to a usage question that was wrong because of `inferred` links a report had
  approved.
- **About 50 lines to calibrate a judge.** A judge wrong 1 time in 10 shows a disagreement in 50
  lines almost surely, and one wrong 1 time in 50 passes clean about 1 time in 3. Evidence that
  changes it: the first calibration. Disagreements that sit in one verdict or one cause move
  the sample there, and a disagreement on a line the judge marked correct raises it, since a
  report of zeros rests on those marks.
- **80% of the sides of a pair.** A cut, with no measurement behind it, that lets the yardsticks
  of HTTP and of package close without every origin. Evidence that changes it: the distribution
  of sides the maintainer reads when writing the order (one origin holds the 80% alone, or the
  cut needs many small origins, each with its own corpus past the floor); a wrong statement
  reported in an origin under the cut.
- **One half, the trigger of ADR 0020's note of this date.** A count of uses from elsewhere in
  the deployment returns to the table when a wider named workspace removes more than half of
  the orphan lines of a narrower one (the lines that vanish, over those plus the lines that
  stay and are real). It is a majority, with no count behind it. It may change only before the
  measurement that reads it, on a count of the same ratio over public code. After that
  measurement it stays, whatever the number was.

The zeros of the thresholds are not on this list. They are not estimates: they say what `exact`
and `ambiguous` mean and what the invariant asks of an absence.

## Considered options

- **A numeric budget inside the invariant.** Not taken: an invariant cannot hold a number nobody
  measured, and CI cannot check it without a labelled corpus.
- **Thresholds fixed after the first measurement.** Not taken: whoever measures would choose the
  target after seeing the number.
- **Rates for every count.** Not taken where the source gives an exact count, and not for open
  misses: on the CI corpus about 34 of 184 calls stay unresolved by ADR 0028's own rule, past any
  rate chosen blind. Causes with expected counts say more.
- **A gate that names HTTP and package.** Not taken: two fixed names cannot be checked against
  the repository, and the 1.3.0 report holds no package item. The criterion replaces them.
- **"Measured noise" alone as the criterion.** Not taken: it sees only where someone looked, and
  no report on a pair is absence, not proof.
- **Only the release waits (no stop on specifications).** Not taken: two unmeasured kinds would
  be in flight at once, on top of links nobody measured.
- **Shipping an unmeasured slice with a note.** Not taken: the floor would mean nothing.
- **Private aggregates counting toward the floor.** Not taken: nobody can check them against
  what the repository holds.

## Consequences

- The maintainer's deciding time goes to yardsticks first. With the yardsticks of HTTP and of
  package still to run and #795 open, no specification of a new kind can be grilled today.
- A slice may stay on its branch after it is built, waiting for a corpus that exercises it.
- Lowering a level to `inferred` until a yardstick approves a rule is a fix, since a wrong
  statement is worse than a missing one, and raising it later is an improvement: the stability
  document does not freeze which links Meada finds.
- The judge and the answer keys live in the measurement harness, outside indexing, so
  invariants 1 and 6 of ADR 0002 hold.
- On acceptance, invariant 7 enters ADR 0002 and the lists of invariants of `AGENTS.md` and of
  `CONTRIBUTING` (in its three languages), and `AGENTS.md` gains the rule that a new contract
  kind or a new origin is specified with its yardstick ticket. Until then those files read as
  before.
