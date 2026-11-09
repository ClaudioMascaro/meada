# Independent cedar and harbor monitoring controls

Status: FROZEN SOURCE ONLY, NOT PROVED. The neutral repository was authored from
#642/#672, ADR0025/0026 and the five retained official native-format documents.
No Meada implementation table, feature fixture or product output was an oracle.
No runtime, indexing, Cargo command, mutation file or auditor file was used.

`repo/` is a separately committed input with twelve source files and 7,587 bytes.
Its synthetic origin is
`https://github.com/independent-monitoring/cedar-harbor-lab.git` and its repository
identity is `independent-monitoring/cedar-harbor-lab`. The origin is only an identity,
never a network source. `input-sha256.json` records all file hashes, Git blobs,
the source tree and the full deterministic commit reconstruction metadata.
The commit is `da43b36c4f43b70c21cef28a6cecdfa751b64318`, tree
`0f90e7861ea569c873a3b6eac86825f1575b264c`, on branch `source-key`.

The recognized source count is eleven alert rules, three recording rules and
fourteen route nodes, 28 in total. Counting the two opaque rule-shaped and two
opaque route-shaped Helm/Secret controls also keeps the authored total at 32.
`expected.json` enumerates all 41 emitters, all 19 matcher instances including
inherited conjunctions, and all 22 selected condition links. Equality conditions
are exact (18); anchored regex conditions are inferred (4), as the specification
states. Every reason retains the full satisfied conjunction, declared condition
source, selected receiver and inheritance facts. These are semantic source facts,
not copied product explanation strings. Route selection is hand-authored from
the input trees and frozen before any product output.

The key covers ordinary YAML and JSON Prometheus/Loki files and YAML/JSON
PrometheusRule objects, group labels overridden by rule labels, literal implicit
alertname applied last, explicit Grafana labels without an invented title label,
and all recording/annotation/metadata negatives. The multi-document bundle uses
`[0]`/`[1]` prefixes. The templated support label overrides its group literal but
is unknown, so it creates no support emitter or guessed receiving link.

Plain Alertmanager configuration exercises match, match_re and scalar matchers.
The JSON AlertmanagerConfig exercises matchType equality, the native legacy regex
flag, inherited conditions, an inherited receiver, ordered siblings and continue.
Critical aurora rules select the first critical child and then aurora-copy;
AuroraPreview selects the regex child and then aurora-copy. The next identical
support route is blocked by first match. QuartzUnknown cannot skip an unknown
zone condition to its later service route. Ledger rules retain already selected
continuing routes while the missing unwritten label blocks a later destination.
Matcher-free fallback roots create no invented alert sides.

Helm values, Chart metadata, malformed Helm templates and Secret-contained rule
and route strings are precise negative sources. There is no Helm rendering,
secret decode, process environment or guessed expression-result label.

`queries.jsonl` contains 151 frozen context, up/down trace and kind-alert search
queries. `query-oracle.json` binds every query to all relevant frozen sides,
links and source spans. Baseline has 25 unpaired emitter instances and 9 unpaired
matcher instances. The source subsets in `phase-plan.json` separately define
emitter-only and matcher-only scopes: each must have no status orphan line, an
explicit per-side missing-counterpart mark, and no alert links. The full input
has both roles in one repository and workspace, so its orphan reports are in scope.

`phase-plan.json` exposes one exact `source_edits` operation for reuse by the
existing capture helper. Replace the single `support: aurora` group literal in
prometheus/aurora.yaml by same-size `support: borneo`. The independently reparsed
`expected-same-size-edit.json` preserves 41 emitters/19 matchers but has 16 links,
removing only the receiving pairs of AuroraCritical and AuroraPreview. Its
query oracle is `query-oracle-same-size-edit.json`.

Public CI must query the warm changed source before export/status, compare all
answers/status/export with an identical cold source, restore exact original
bytes and metadata, and recapture the full restored key. Run identical frozen
queries over HTTP and stdio in each exact scope/phase and compare complete answers
byte-for-byte. Baseline, edit, cold comparison, restoration, role-only scope
captures and HTTP parity remain runtime obligations until captured and separately
judged. The input commit and source keys are preparation, never a passing receipt.
