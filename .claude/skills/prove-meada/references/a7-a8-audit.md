# Bounded A7/A8 audit: #676 and #684

Use `/audit-prove-it` and `/maintain-prove-it` with the existing skill. The source/spec keys
remain #642/#676, the child flow issues, #683/#684, and base ADRs 0025/0026. The executable
plan is [a7-a8-mutations.json](../scripts/a7-a8-mutations.json): eight plausible A7 bugs across
values, IaC nodes, provisioners, workload environments, Service DNS, alert routing, AWS direct
code and GCP subscription identities; five separate rounds cover all five original A8 controls.
The two inline config controls exercise different mapping layers. Additional A8 controls
remain regressions. SDK/route flows added later retain their own receipts and Scope rows;
this bounded mix does not claim to mutate every child flow.

The `Proof` workflow (`.github/workflows/proof.yml`) runs `proof-a7-a8` only through explicit
`workflow_dispatch` phases: `baseline`, `candidate`, `check` and `final`. Ordinary PR/push runs
use the routine gates of `CI` (`ci.yml`) and, on a labeled PR, the selected milestone E2E of
`Proof`; they run neither this audit's baseline nor mutations.
The manual baseline's 26 named native checks include the full AWS subscription/provision/orphan
flow separately from JSON SSM and generic provisioning, GCP/Azure and JVM/Go/.NET AWS SDK
drivers, IaC HTTP routes, custom mappings, declared resources, orphans, patterns and image regressions. The executable
`commands()` inventory fixes their distinct evidence directories. The supported IaC oracle
also covers the existing public `terraform-sqs` and `sam-rust` pins. The e6d27d0 baseline failed
two of 15 checks; the 2669d15 baseline failed two of 16 through the later A8 product regression.
Both remain BROKEN and cannot authorize mutations.
Full validation policy lives in [AGENTS.md](../../../../AGENTS.md). The manual phase inputs
reuse that workflow; there is no second environment or workflow guide. If GitHub cannot
dispatch a phase on the branch, leave it BLOCKED, never infer a pass from a local dry run.

## Baseline and phase boundaries

`a7-a8-audit.py baseline` builds immutable default-feature binaries for the exact checkout,
its merge-base with the associated open PR's actual base SHA and the three existing drivers'
historical bases below. The workflow resolves exactly one open PR for the repository owner
and dispatched branch, validates its base SHA and computes that merge-base. Missing or
ambiguous PRs block the baseline. It copies each binary before releasing the compiler lock,
pins every skill/answer-key byte and drives the fixed source-derived checks. `baseline.json` records the
source commit/tree, actual build commits, binary SHA256s, keys, flows and raw check results.
At the baseline the PR merge-base binary is the key of the existing flows, so the clean head is
checked against the base; later phases take existing A7 annotation/whole-export bytes from the
accepted clean A7 binary. Independent source/spec oracles supply new A7 semantics. A8 additionally compares the original base on
identical generated input. HTTP parity supplements these keys. It never replaces them.

| Packet binary | Purpose and immutable source |
|---|---|
| `bin/meada-clean` | exact frozen head, existing A7 annotation/export key |
| `bin/meada-base` | actual PR merge-base, existing-flow key at the baseline, A8 original fields and corpus |
| `bin/meada-pre-config` | before #661: `6a5eca5853193d205bcf756983b41e9cb16ffa9d` |
| `bin/meada-pre-image` | before #662: `eae2a567ed250d0ac4a8b8e6f57b62f7a3041c62` |
| `bin/meada-pre-extensions` | compatible A7 before #666/#668/#670/#671: `e6d27d0d7c046eceb8771f17111f00511a8d6c3d` |

`Proof`'s `e2e-frontend` job depends only on `e2e-scope` and also builds/uploads the
exact dispatched source for an explicit manual baseline. The separate `proof-a7-a8-ui` job
runs only after that frontend and `proof-a7-a8` baseline both succeed. It checks out the same
source SHA, downloads that frontend and baseline manifest, and prepares the pinned browser
tools and UI binary. `ui-check` drives the full `aws-sdk.py`, `config_uses.py` and
`images.py --public` flows with their required bases and UI flags, beside the existing #675 graph check. `a7-a8-ui` retains its immutable
`bin/meada-ui`, `frontend/` bytes, raw captures and a receipt bound to the actual source/tree, complete verifier
digest, baseline manifest, UI binary and frontend bytes. This is clean final-source evidence,
not coverage of a native mutant. None of the 13 required mutations needs a UI candidate.

`scope-check` uses the existing corpus loader's seven repositories (eight language packs),
then compares UI-feature stdio to the native captures before one-entry console API checks
and the existing viewer pages/budgets. It retains full base/head records, queries and HTTP
captures, every API/stdio answer, page screenshots, cleanup results and source/key/binary
identity in `a7-a8-scope`. A corpus comparison exit 1 stays visible as a raw difference,
including its budget/determinism evidence; it requires fresh source/spec judgment, not a
replacement snapshot. Only this comparison can differ while the capture phase completes.
API, viewer, feature-parity and cleanup failures fail the capture. `CAPTURED` is never PROVED.

The same artifact captures nine exact source scopes from `scripts/a7-source-keys/`. Their
frozen hashes, neutral Git identities and symlinks are verified before execution; public
source copies are fetched at the unchanged existing pins. The independent keys supply:

- native HCL: 72 whole value rows and 38 queries, with literal/sensitivity edits;
- complete pinned monitoring: 808 queries, preserving the original 434-query preparation;
- adjacent monitoring: 151 queries, one label edit and both role-only scopes;
- six generic IaC/Serverless scopes: 460 nodes, 221 reference groups and 1,509 queries, with
  the exact per-repository query files from `generic-iac/capture-plan.json`.

`a7-source-capture.py` uses the existing transports for the eleven frozen source edits:
ten same-size edits and one explicit Compose override clear. The persistent MCP reader
queries before CLI export/status. Edits change mtime even when size stays fixed, following
ADR 0004's stat-based freshness contract; simultaneous preservation of size and mtime is
outside that guarantee. Each phase retains warm/cold and restored warm/cold
answers, full responses/status/export, same-scope HTTP replies and exact copy/cmp/hash/mtime
restoration. Only JSON-RPC IDs are excluded from whole response comparisons; raw replies
remain intact. Explicit HCL privacy constraints inspect responses and diagnostics, excluding
request arguments and input/key files. Only an exact successful search answer of
`no matches for <the caller's query>` excludes its query echo from the privacy view;
all other response fields, diagnostics and the original raw answer remain inspected or retained.
The verifier self-check includes real payload, extra-content, metadata and error controls.
Source CAPTURED is never semantic acceptance.

Read the frozen source-key `PLAN.md` files and HCL `ASSUMPTIONS.md` for their independent
expectations and limits. The separate IaC var-file counter is unadjudicated; native type
subfields that cannot be observed on a documented public surface stay UNPROVEN. Complete
source semantics, corpus deltas/budgets and supplementary SDK controls still require fresh
source-key/receipt judgment. A YAML/JSON value oracle, curated contract mapping or transport
pass does not supply that judgment. The old partial preparation keys remain unchanged.

A script result is `CHECKS_PASSED`, `BROKEN` or `BLOCKED`, never `PROVED`. A fresh checker must
judge the untouched named flows, source/spec keys, full captures and limits first. Retain that
receipt and provide its exact manifest digest in the `baseline_attestation` input:

```json
{
  "verdict": "PROVED",
  "checker": "actual fresh checker identity",
  "receipt": "actual retained receipt URL/path",
  "baseline_manifest_sha256": "actual baseline.json SHA256",
  "ui_receipt_sha256": "actual a7-a8-ui/receipt.json SHA256",
  "scope_receipt_sha256": "actual a7-a8-scope/receipt.json SHA256",
  "separate_scope_receipt": {
    "receipt": "actual independent source-scope judgment URL/path",
    "sha256": "actual judgment SHA256",
    "verdict": "PROVED",
    "covers": ["corpus-api-viewer", "public-infra", "native-hcl", "serverless", "generic-iac", "supplementary-sdk"],
    "source_commit": "actual frozen source commit",
    "verifier_keyset_sha256": "actual canonical full verifier inventory SHA256"
  }
}
```

This is an evidence gate, not cryptographic identity verification. Supplying invented approval
does not establish a baseline. Any source or frozen-key change requires a new baseline and
fresh judgment. Old BROKEN, unfinished and refused receipts remain intact.

When the user requires inline execution or fresh agents are unavailable, use the scripted
fallback in `audit-prove-it`: record `checker_mode: "inline"` in the actual baseline, round
and final judgments, and `fresh_blind_checker: false` in every round. The same agent still
judges the complete source/spec, native, UI and scope evidence. Run `report --not-blind`
only for that mode. It requires all 13 named script detections, BROKEN symptom judgments,
exact restoration and the final clean bound judgment, and reports `COMPLETE_NOT_BLIND`.
Blind rounds remain incomplete and the fresh/blind counts remain zero. Omitting this flag
keeps the independent completion requirements; an inline judgment never supplies independence.

A successful explicit `baseline` dispatch produces all three bound packets: `a7-a8-baseline`,
`a7-a8-ui` and `a7-a8-scope`. Failed captures retain their available artifacts and fail the job;
a skipped job supplies no evidence. Later phases reuse the frozen baseline and UI/scope
packets, with `ui_run` defaulting to `baseline_run`. A separately produced supplement must
match that exact manifest. A same-source claim without the actual matching receipts cannot
authorize a candidate. Captures and script checks still leave A7/A8 acceptance pending until
the required source/spec and receipt judgments are retained.

1. Dispatch `candidate` with the frozen `baseline_run`, one `mutation` ID and the actual
   baseline attestation and bound UI/scope artifacts. The isolated CI checkout must be clean. One exact anchor is required;
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
   --ui-receipt UI/receipt.json --scope-receipt SCOPE/receipt.json --output report.json`
   retains chronological attempts, pending rounds, both catch counts,
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
