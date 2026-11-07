# Bounded A7/A8 audit: #676 and #684

Use `/audit-prove-it` and `/maintain-prove-it` with the existing skill. The source/spec keys
remain #642/#676, the child flow issues, #683/#684, and base ADRs 0025/0026. The executable
plan is [a7-a8-mutations.json](../scripts/a7-a8-mutations.json): eight plausible A7 bugs across
values, IaC nodes, provisioners, workload environments, Service DNS, alert routing, AWS direct
code and GCP subscription identities; five separate rounds cover all five original A8 controls.
The two inline config controls exercise different mapping layers. Additional A8 controls
remain regressions. SDK/route flows added later retain their own receipts and Scope rows;
this bounded mix does not claim to mutate every child flow.

The existing `CI` workflow's `proof-a7-a8` job runs the untouched baseline on PRs/pushes.
Its 16 named checks include the full AWS subscription/provision/orphan flow separately from
the JSON SSM and generic provisioning checks. The first e6d27d0 CI baseline covered only
15 checks and failed two; it is retained as BROKEN and cannot authorize mutations.
Full validation policy lives in [AGENTS.md](../../../../AGENTS.md). The manual phase inputs
reuse that workflow; there is no second environment or workflow guide. If GitHub cannot
dispatch a phase on the branch, leave it BLOCKED, never infer a pass from a local dry run.

## Baseline and phase boundaries

`a7-a8-audit.py baseline` builds immutable default-feature binaries for the exact checkout
and its merge-base with main, copies each before releasing the compiler lock, pins every
skill/answer-key byte and drives the fixed source-derived checks. `baseline.json` records the
source commit/tree, actual build commits, binary SHA256s, keys, flows and raw check results.
The accepted clean A7 binary supplies existing A7 annotation/whole-export bytes; independent
source/spec oracles supply new A7 semantics. A8 additionally compares the original base on
identical generated input. HTTP parity supplements these keys. It never replaces them.

A script result is `CHECKS_PASSED`, `BROKEN` or `BLOCKED`, never `PROVED`. A fresh checker must
judge the untouched named flows, source/spec keys, full captures and limits first. Retain that
receipt and provide its exact manifest digest in the `baseline_attestation` input:

```json
{"verdict":"PROVED","checker":"actual fresh checker identity","receipt":"actual retained receipt URL/path","baseline_manifest_sha256":"64 hexadecimal characters from baseline.json"}
```

This is an evidence gate, not cryptographic identity verification. Supplying invented approval
does not establish a baseline. Any source or frozen-key change requires a new baseline and
fresh judgment. Old BROKEN, unfinished and refused receipts remain intact.

1. Dispatch `candidate` with the frozen `baseline_run`, one `mutation` ID and the actual
   baseline attestation. The isolated CI checkout must be clean. One exact anchor is required;
   drift blocks the round. It copies bytes to `.prove-it/audits/.../originals/<id>/<path>`,
   plants one semantic bug, builds one binary, then restores by copy in `finally`. Actual
   `cmp`, every tracked-file SHA256 and complete git status must match before publication.
   A failed compile is BLOCKED, not a catch. No mutant is committed, stashed or pushed.
2. `a7-a8-candidate` contains only the binary and opaque provenance. `a7-a8-auditor` separately
   retains originals, mutation identity/patch, compiler output and exact restore receipt.
   The fresh checker gets only the candidate packet, clean baseline packet, named flows and
   keys. It must not read auditor artifacts, workflow inputs/logs, the mutation plan, git diff,
   status or history. Public separation alone does not make a checker blind.
3. Dispatch `check` with only `baseline_run` and `candidate_run`. The frozen scripts drive the
   candidate and retain every stdout/stderr/result and full response even when a check fails.
   A fresh checker records BROKEN/PROVED and the observed symptom separately. Do not start the
   next candidate until this round is judged and the restore receipt is confirmed.
4. Run `score --auditor DIR --checks DIR --judgment FILE --output FILE`. Judgment JSON names
   `verdict`, `checker`, `receipt`, a verbatim symptom excerpt in `symptom`, a relative raw
   `evidence` file, `script_names_symptom` and
   `fresh_blind_checker`. An expected check's exit 1 plus evidence naming the symptom can be a
   script detection, recorded separately. A blind catch requires the actual fresh checker's
   BROKEN verdict with the named symptom. PROVED is a miss even when scripts detected it;
   script-only or BLOCKED/UNPROVEN judgments leave the round incomplete. Unrelated BROKEN
   evidence is luck. Aggregate reporting revalidates these judgments rather than trusting
   derived catch flags. Ask a fresh adjudicator when
   uncertain. Retain the original miss, fix only the authorized verifier/key gap, baseline the
   new frozen keys, and replant it. A missing product behavior is a finding, never a new key.
5. Dispatch `final` on the same exact source/keys after all rounds. Judge the clean raw results
   afresh. `report --baseline DIR --rounds SCORE... --final checks.json --final-judgment FILE
   --output report.json` retains chronological attempts, pending rounds, both catch counts,
   blind counts and final false alarms. Its COMPLETE state requires all 13 current rounds,
   fresh blind judgments and an actual final clean PROVED receipt for that manifest. It is
   still a bounded audit, not whole-app acceptance.

Every artifact is retained for 30 days. Download the baseline, auditor, checker and reports
outside git before expiry and link their public CI runs from PR evidence. The candidate
phase's safety harness uses a disposable dummy repository, including failed-build restoration
of CRLF bytes and refusal of changed keys/binaries. It is not a product mutation score.

## Maintenance evidence and limits

The A8 oracle constructs the entire expected export from the trusted base plus specified
resolutions, including terminal process steps. It never drops whole landed consumers, links
or processes. Unexpected fields, locations, second calls, process metadata and extra steps
must fail. `a8-mcp.py` adds complete HTTP/stdio reply parity, direct context/trace provenance,
closed-item checks, ordinary HTTP base-answer preservation, and partial-only content refresh
with context as the first reader after both edit and restoration. Context and trace each
assert the source-selected API_URL/ALT_URL reason and peer, with warm/cold full-export
equality and exact restoration.

Only real declared environment peers require the adopted #683 context explanation. Unmapped
proxy contexts stay byte-equal to the trusted existing inline proxy control (with the neutral
workspace identity substituted for new shapes); complete proxy trace and link reasons remain
required. A changed proxy context is a regression, not an occasion to add a new explanation.
Value-budget probes derive a genuinely cutting cap from independent source-row UTF-8 bytes;
complete answers that fit their requested cap correctly have no cut note.

Generic IaC preserves existing annotation and contract-role tails without taking independent
identity/type/root/name expectations from the head. The Terraform oracle reads an S3 name
from its provider-native `bucket` field. An unsupported name expression remains explicitly
unchecked and uses a trusted existing base answer, never a guessed new name. CloudFormation
NoEcho defaults are identified from source keys and redacted directly; status counts add only
defaults the base did not already redact. Recognition requires a CFN header or SAM transform
and a Resources mapping: missing/nonmapping Resources stay ordinary public config. YAML
aliases in parameter/default/flag fields are unwrapped for privacy while value locations keep
their alias-use lines. Live positive and negative controls protect both failure directions.
Native HCL values remain outside the YAML/JSON value
oracle; the dedicated SSM/provisioner drivers cover their scoped semantics.

`incident-guards.py` retains the independent #673 dynamic AND/OR and first-match count,
for_each, unknown/different-team and literal controls. Historical #641/#665/JSON/alert Engine
RED/GREEN records suggest mutations but do not count as fresh skill catches. Captured-response
faults prove comparator maintenance only. Keep those records separate from actual product
mutation, script catches and fresh blind judgments. Final integration still requires the
current final source and all other scoped receipts; older partial evidence cannot promote it.
