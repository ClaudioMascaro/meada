# Complete immutable kube-prometheus monitoring source key

Status: FROZEN SOURCE ONLY, NOT PROVED. No product output, Meada implementation
tables, feature fixtures, mutation files or auditor files supplied expectations.
No binary, Cargo command or indexing was run during this preparation.

The source remains prometheus-operator/kube-prometheus at
`b6c2f62b287524652592e6e2f7ea0d3054cb8ac6`. The retained complete Git tree has
358 entries and is not truncated. All 133 YAML/JSON entries and both .gitignore
files are retained under `source/`. Their 135 byte lengths, Git blob SHA1s and
SHA256s are verified in `input-sha256.json`. The original thirteen retained files
were copied only after verifying their original hashes; the missing sources were
obtained by immutable GitHub blob GET. `acquisition.json` records the unchanged
original prep evidence. `missing-blobs.json` is the historical acquisition list,
not a current missing-source report.

The independent walk applies ADR0025: .github is allowed; .gitpod.yml and
.mdox.validate.yaml are hidden-path exclusions. Neither .gitignore matches any
selected YAML/JSON file, and the complete tree contains no .ignore or
.meadaignore. All selected source entries are ordinary files. The 1,053,739-byte
grafana-dashboardDefinitions.yaml exceeds the approved 1 MiB ceiling and creates
no monitoring facts. The malformed alertmanager-config-with-template.yaml is
unreadable at line 21. There are 129 syntax-readable allowed files containing
135 documents. The source reader uses YAML syntax nodes, avoiding the unrelated
PyYAML constructor failure on a CRD's native enum value `=`.

Eleven documents actually contain monitoring rules or routes. No additional
recognized document exists beyond the original source selection: eight
PrometheusRule objects, both distinct existingrule files, and the public plain
Alertmanager config. `expected.json` records all 141 alert rules, 290 emitter
instances, 96 recording-rule negatives, two route nodes and the one matcher.
Every site includes its file, document, canonical key and scalar source span;
inherited/implicit label origins retain their scalar key and line.

The only receiving link is exact:

- `manifests/kubePrometheus-prometheusRule.yaml::spec.groups[0].rules[1]`,
  implicit alertname Watchdog at line 25, reaches
  `examples/alertmanager-config.yaml::route.routes[0]`, equality matcher at
  line 13, receiver `null` at line 14.

No matcher-free root side is invented. Recording rules, annotations and object
metadata are precise negatives. Ten CRDs remain definitions; five Secret scalar
payloads remain opaque, with no decode. Unknown expression-result labels supply
no guessed routing condition. The 920 negative source records retain the exact
source keys/lines or document spans and reasons.

`queries.jsonl` contains 808 independently chosen queries, including all original
434 query objects unchanged. `query-oracle.json` binds every query to the frozen
source facts, full side/link indices and both trace directions. Alert searches
explicitly use kind alert, so negative source strings cannot be mistaken for
config-value fallbacks. Compare the complete exported alert side/link multisets,
full source locations and full query results. There are 289 unpaired emitter
instances and no unpaired matcher instance in this source scope. Status orphan
noise is allowed only because both alert roles exist in that scope.

Public-CI capture must retain the exact pinned checkout identity and whole
status/export, run these frozen queries over stdio and HTTP in the same scope,
and compare complete answers byte-for-byte for transport parity. Final source
and source-key identities must be bound before a separate independent checker
judges the raw capture. Source preparation and CAPTURED never mean PROVED.

Native format authorities and immutable document hashes are retained separately
under `../monitoring-format-authority/`; #642, #672 and ADR0025/0026 govern the
Meada-specific scope. Existing prep and prior receipts are not overwritten.
