# Binary comparisons in Proof

A run identifies the driver revision and the selected merge-base and head in `receipt.json`.
The command joins each step to its actual binaries. The receipt and `bin/*.from` identify
the revisions, features and executable hashes, including references built from fixed commits.
The source and answer key for a step are in its Scope row and its own evidence directory.

| Driver | Comparison binary | What to judge |
|---|---|---|
| `prove.py run`, `values`, `iac` | PR merge-base | Full exports and sampled MCP, independent config/IaC keys and budgets. Exit 1 is a comparison, 2 is blocked, 3 rejects the answer. |
| `http-iac.py` | PR merge-base | Direct-host control and whole unchanged export, beside the independent route and stored-query checks. |
| `iac-http-routes.py` | PR merge-base, audit extension tier when run manually | Existing code HTTP export and display, with and without source. Independently check IaC routes, fallback and both transports. |
| `aws.py` | PR merge-base | Code-only export, status, MCP answers and catalog, beside independent AWS and NoEcho keys. JSON-values uses the head alone. |
| `aws-java-private-fields.py` | PR merge-base and `b73e1e5318508d69a8e98e1081e2e9fb3c61076e` | Each PR binary against the five Java-oracle rows. The explicitly selected pre-fix revision has its own five historical rows. |
| `orphans.py` | PR merge-base | Whole status, export and MCP comparison. The exact orphan sets, scope isolation, mutations and current transport parity have independent checks. Manual callers can select a base before #657. |
| `incident-routing.py` | PR merge-base | Whole existing alert export. Incident clauses and mutation/transport probes remain independent. |
| `images-mixed.py` | PR merge-base, AWS-capable older tier in the manual audit | Complete non-image row controls, beside independently expected mixed image edges and rollback. |
| `ci.py` (F1, F2, F3 and corpus) | PR merge-base | Full exports outside the explicitly source-owned CI rows and existing query controls. New CI sides have source keys. |
| `a8-peers.sh check` | PR merge-base (positional) | 44 table rows and complete exports against a base plus the specified #683 changes. `open_items` remains in the export. |
| `a8-mcp.py` | PR merge-base | Ordinary MCP catalog and query comparisons, beside source-keyed proxy links and refresh/restore. |
| `aws-sdk.py` | PR merge-base | Code-only SQS export, status, MCP answers and catalog, beside the independent SDK and console checks. |
| `config_uses.py` | `6a5eca5853193d205bcf756983b41e9cb16ffa9d` | The release before #661 is the source of the pre-feature parity keys. Keep the raw historical catalog, exports and status when #855 or later MCP wording changes them. |
| `images.py` | `eae2a567ed250d0ac4a8b8e6f57b62f7a3041c62` | The release before #662 is the source of existing non-image rows. The pinned Boutique input also has source-derived keys for the new image behavior. |
| `custom-iac.py` | `e6d27d0` in the manual audit, no base in PR Proof | The historical oracle requires no provisioner. Current source-keyed package, channel and workload checks are independent. |
| `smoke_stack.py` | PR merge-base for resolution and lifecycle, plus named reference revisions for upgrade and SQS | The public corpus pins are in the receipt. Scope follows the source, running-probe transport records and complete per-site expected answers. #941 and #943 supply the historical upgrade pair. 68f1c0b supplies the SQS extraction control. |
| `release-diff.py` | Previous release tag, Release diff workflow | Exports and orphan policy deltas are retained for the release reviewer. It exits 0 with a diff and 2 on a capture failure. |

The PR comparison drivers use exit 4 for a comparison delta, 3 for a rejected oracle and 2
when execution was blocked. The runner separately records timeout 124 and its job deadline.
`comparisons.jsonl` identifies each comparison and its expected, actual and unified diff
files when they differ. The A8 shell driver keeps its own `peers.md` and full export diffs.
Some older drivers use their own checks JSON. An oracle failure still fails capture when
the same driver also found comparison deltas. Later independently runnable drivers still
execute. A delta can be a regression. Judge the complete recorded values against the source.

`prove.py run` and `rebless` use the same JSON reader. They keep new and unknown fields,
including an empty array that was previously absent, and report new, removed or moved
candidates by decision identity. Arrays keep their multiplicity and order. The reports
bound examples per field and the original exports and tracked goldens retain all rows.
