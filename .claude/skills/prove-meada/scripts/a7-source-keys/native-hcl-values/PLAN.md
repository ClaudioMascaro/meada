# Independent native HCL whole-value key

Prepared for PR #711 from source `fe0faf5dfd7408571bf982bc395ef10d1e456a0b`.
This directory is a frozen source key, not a runtime receipt or a product verdict.
No Meada binary, Cargo command, product answer, index database, Terraform CLI,
provider, process environment, implementation mapping table, or evaluator was used
to author its expected rows. `author-key.py` writes declared source and expectations;
it is not a new capture harness. Only source syntax was parsed independently.

The neutral fixture is `repo/`, with origin
`https://example.com/a7-proof/native-hcl-values.git` and repository identity
`example.com/a7-proof/native-hcl-values`. Preserve that exact origin when reconstructing
Git metadata in CI. Its original commit/tree and complete tracked-file hashes are
recorded in `input-sha256.json`. All 72 expected rows contain the exact key component
`proof`; the exhaustive kind-value query uses budget 200000. There are 38 queries:
31 searches and 7 contexts. The 14 HCL/var files plus `meada.toml` total 6,607 bytes.

`expected-values.json` contains every baseline row's file, canonical key, 1-based
source line, native type, value, cut/redacted flags and entire rendered line. It also
contains all source-selected query results in exact rank/path/line/key order,
omitted null/empty/unreadable/shadowed rows, phase rows, status counts, privacy and
export expectations, declared target metadata, and exact same-size edits/hashes.
Do not drop unknown rows, fields, diagnostics or payloads into a passing comparison.

## Independent source basis

- `references/issue-639.json`, `issue-640.json`, `issue-641.md`, `issue-642.md`:
  value search and rank, native HCL, redaction, status and declared target context.
- `references/adr-0025.md`, from the root source: config row/key grammar; repeated
  blocks with the same type/labels; literal collection leaves; source-preserved
  expressions/templates; repository-wide sensitive names; unreadable-ancestor
  conservatism; tofu shadow exclusion; native bracket ceiling.
- `references/configuration-values-ceilings.md`, from the same source:
  last UTF-8 boundary within 1024 bytes, visible cut marker, numbers stored as written,
  line breaks in expressions/templates displayed as `\n`, and 64 open brackets
  counted even in strings. `depth/at.tf` reaches 64; `depth/over.tf` reaches 65.
- `references/user-guide.en.md`, from the same source: complete search rendering,
  JSON string quoting, cut footer and four-byte token estimate, exact config status
  grammar, known scope of native HCL and declared targets.
- Pinned official HCL native syntax and language specifications at
  `hashicorp/hcl@94e232b58b12531a80299e3aa7566f345e17c21b`: numeric literal grammar,
  unary expression syntax, quoted keys, source expressions, string escapes,
  interpolation/directives, heredoc newlines and indentation.
- Pinned OpenTofu files documentation at
  `opentofu/opentofu@c570669d0fed5787f18d2f017f56b2658e2e6cce`: extension precedence.

Exact bytes, URLs and SHA256s of these sources are in `input-sha256.json`.

## Capture handoff to owner a7_676

Only a7_676 copies/wires these exact keys into tracked CI/runner files. Reconstruct
the fixture repository with its recorded origin before indexing. Use the existing
public-CI capture path, for example its existing command shape:

```text
capture-source.py --head-bin <final-H-binary> --source <frozen-repo>
  --name native-hcl-values --queries <frozen-queries.jsonl> --run <new-artifact-dir>
```

Keep the exact final root source, executable SHA256, query/key/reference SHA256s,
all actual fixture hashes and transport scope in the raw artifact. This helper
only captures a baseline; it does not prove scalar semantics or the edit lifecycle.
Use the existing runner/transport helpers for the following lifecycle. Do not
replace it with a newly invented harness.

1. Verify all frozen bytes, copy the fixture, and cold-index the baseline. Save full
   index/status/export stdout, stderr and exits, tool schemas, all 38 complete stdio
   answers, and copies over HTTP under exactly the same repository/scope/arguments.
2. In that already indexed working copy, replace `refresh = "maples"` with
   `refresh = "willow"` exactly once in `botany.tf`. The byte size stays fixed;
   preserve and reset its original `mtime_ns` before the warm query/refresh. Save
   the whole warm status/export/38-query set. A separate cold copy with exactly
   these changed bytes must have identical answers/status/export after excluding
   only predeclared transport envelopes, never semantic fields.
3. Restore exact baseline bytes and mtime; verify the complete fixture hashes and
   baseline query/status/export equivalence before the next edit.
4. Starting from baseline, replace `sensitive = true ` with `sensitive = false`
   exactly once in `sensory/variables.tf`, again preserving size and original
   mtime. Warm and cold captures must each equal `sensitivity_public`: 72 rows,
   3 redacted. Unchanged `visible/*.tfvars` must refresh to public rows, while all
   3 rows under `opaque/child` remain redacted. The ignored `shadow/fern.tf`
   sensitive marker must not keep the visible var files redacted.
5. Restore exact baseline bytes and mtime again. Verify every original source
   hash, all complete query answers, status and export; preserve the restoration
   record and HTTP server cleanup evidence.

Both edits have exact before/after text, lengths and SHA256s in the key. Phase
expectations start independently from baseline. The literal edit changes one row;
the sensitivity edit changes its marker and 11 redacted rows, preserving every
other row/field. The config status is exactly 11 read, 2 skipped/unreadable,
0 lockfiles, 72 values; redacted is 14/14/3/14 for baseline/literal/public/restored.

The exhaustive query must match all 72 rows and their listed order with no cut.
The one-token query must retain zero complete rows and the documented 0-of-72
cut footer. Exact/prefix/substring and two-character scan controls have complete
ordered keys. Source secret substrings must be absent from complete responses,
CLI diagnostics and exports according to each phase. Request arguments retained
in a transcript are not secret-response leaks. Values remain absent from export,
and the three explicit declared key targets keep payload privacy in their contexts.

All raw context/status/export/decision payloads remain reviewable in full. The
separate IaC var-file counting convention and unobservable explicit kind fields
remain unadjudicated as described in `ASSUMPTIONS.md`; they cannot become a pass
by copying candidate output. The dedicated SSM receipt retains its own matrix.
Final runtime judgment waits fresh exact-source/key-bound CI artifacts.
