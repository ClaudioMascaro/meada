# A7: config values and IaC (spec #642)

The A7 section of prove-meada. Spec #642 ("Config and IaC in the graph") is the answer key of
every new A7 flow: its user stories, read at run time from the issue
(`gh api repos/ClaudioMascaro/meada/issues/642 --jq .body`), never from a PR body or a golden the
same change wrote. The base binary is the answer key of everything an A7 change leaves as it
was, a repository with no config included.

## Flows

Each A7 ticket that adds a flow adds its row here, its row in SKILL.md's Scope table, and its
check (a `prove.py` subcommand or `--queries` file) in the same PR. A flow whose check does not
exist yet is `pending`: a change that reaches it is UNPROVEN on that flow until the ticket that
lands it writes the check.

| Flow | Stories | Surfaces | Entries (`scripts/infra.tsv`, `scripts/corpus.tsv`) | Check | State |
|---|---|---|---|---|---|
| Config values: `search` kind `value`, the default search's fallback, `meada status`'s config line, values kept out of the export | 1 to 17 | CLI (`meada status`, `meada export`), MCP stdio, MCP HTTP | `a7-probe` and every `infra.tsv` entry, plus `meada` | `prove.py values --http` | landed (#639) |
| IaC nodes, Terraform: `search` kind `iac`, `context` and `trace` both ways over references, nodes kept out of the default search and the export | #649's criteria (of 18 to 31) | MCP stdio and HTTP, CLI (`meada status`, `meada export`) | `a7-iac`, `terraform-sqs` | `prove.py iac --http` | landed (#649) |
| Static evaluation of Terraform names: evaluated names and patterns in `search` and `context`, the `overridable:` section, `count` and `for_each` over values that resolve | #650's criteria | MCP stdio and HTTP | `a7-names`, `a7-iac`, `terraform-sqs` | `prove.py iac --http` (its oracle `tf_names.py`) | landed (#650) |
| IaC nodes, CloudFormation and SAM: `search` kind `iac`, `context` and `trace` on a logical id, `Ref`, `Fn::GetAtt`, `Fn::Sub` and `DependsOn` references, names evaluated within the template (a parameter's default as `overridable:`, pseudo parameters and imports as patterns), `NoEcho` | #653's criteria (of 18 to 31) | MCP stdio and HTTP, CLI (`meada status`, `meada export`) | `a7-cfn`, `sam-rust` | `prove.py iac --http` (its oracle `cfn_oracle.py`) | landed (#653) |
| IaC nodes, Serverless (#654): `search` kind `iac`, `context` and `trace` on `functions.<name>` and on the `resources` section's logical ids, handler and events, `${self:...}` and `${opt:...}` resolved, `${env:...}` and `${ssm:...}` as patterns | #654's criteria (of 18 to 31) | MCP stdio and HTTP | `a7-sls` | by hand against the probe's README (no oracle yet: a follow-up) | landed, unchecked by script (#654) |
| IaC nodes of the other formats (Kubernetes, Kustomize, Compose, Crossplane) | 18 to 31 | MCP stdio and HTTP, CLI status | `boutique`, `crossplane-aws`, `kube-prometheus` | to write | pending |
| Terraform SQS/S3 provisioners, bounded patterns, levels, root labels, context and trace | #656 | CLI export, MCP stdio and HTTP | neutral generated workspace | `provisions.py --head-bin BIN --run DIR` | landed (#656) |
| AWS SNS/EventBridge/Kinesis, DynamoDB/RDS/ElastiCache/SSM/Secrets Manager provisioners, ARN extraction, SecureString and SNS/EventBridge subscriptions | #658, #642 32 to 43 and 57 to 60 | CLI export/status, MCP stdio and HTTP | independently generated neutral AWS workspace | `aws.py --head-bin BIN --base-bin BASE --run DIR` and `aws.py --json-values --head-bin BIN --run DIR` ([aws.md](aws.md)) | landed (#658) |
| Scope-aware orphans: status only when both sides of the contract kind plus broker/resource kind exist; otherwise side markers in search and context | #657; #642 stories 59, 60; ADR 0026 Orphans | CLI status/export, MCP stdio and HTTP; existing shared queries also console API | neutral independently generated repositories, hashed per run | `orphans.py --base-bin BASE --head-bin HEAD --run DIR --http`, plus all-corpus base parity and console API | landed (#657) |
| Infrastructure contracts: SDK and config use sides, subscriptions | 32 to 43, 57, 58 | CLI export and status, MCP | `terraform-sqs`, `sam-rust`, `crossplane-aws` | to write | pending |
| Workloads, image contract, environment entries | 44 to 49 | CLI export, MCP | `boutique`, `sam-rust` | to write | pending |
| HTTP through IaC: Services, Ingress, IaC-declared routes | 50 to 52 | CLI export, MCP | `boutique`, `sam-rust` | to write | pending |
| Alert routing | 53 to 56 | CLI export, MCP | `kube-prometheus` | to write | pending |
| `via` to an IaC address or a key path | 61, 62 | CLI status | `a7-probe` with an overlay | to write (#641) | pending |
| Export and console graph additions | 63, 64 | CLI export, console | every IaC entry | to write | pending |

## `orphans.py`: #657

```bash
python3 $S/orphans.py --base-bin "$RUN/bin/meada-base" --head-bin "$RUN/bin/meada-head" \
  --run "$RUN/orphans" --http
```

When the brief's exact baseline already contains #657, add `--base-has-orphans`:
the complete status, including its orphan rows, must equal that baseline. The
script still independently asserts the source-derived orphan set and row count.

When the six scenarios were already completed and only the same-token cross-kind control
needs adding, the same command with `--cross-kind-only` drives just that focused addendum in
its own run directory, with its own input hashes and captures.

Extend this existing A7 section through `/create-prove-it`, never a second prove skill. The
answer key is the maintainer's #657 acceptance criteria and ADR 0026 at the required base,
read before checking. #642 supplies the parent contract model. Neither feature tests, their
fixtures, newly written docs nor the implementation are answer keys.

The script generates a neutral input tree independently of feature fixtures, stages files
without making commits, saves a SHA256 inventory, and copies exactly those bytes to the base
and head drives. It saves every CLI stdout/stderr/exit status and query/answer, not just a
summary. A script pass is input to the independent checker's verdict.

| Method | Surface | Answer key | Catches | Misses | Cost |
|---|---|---|---|---|---|
| Whole status capture on mixed workspace, code-only and IaC-only repositories | CLI | #657 (4), unchanged lines against exact base (1) | extra/missing orphan lines; absent-side noise; changed legacy diagnostics | unseeded groups | under 1 minute after build |
| New-flow queries and transport parity | MCP stdio and HTTP | #657/base ADR 0026 (4), HTTP equal to stdio (1) | missing markers on contract/default/IaC search, contract/node/symbol context; scope leaks | authenticated grants unless separately driven | under 1 minute |
| Same-size name edit and reverse on persistent stdio server | MCP and CLI | #657 (4); before/after state | stale orphan computation and stale restored state | server-mode refresh, which is explicit reindexing | seconds |
| Existing-flow base parity over every corpus entry | CLI export, MCP stdio and HTTP, console query API | exact base on same pinned input (1), console equals stdio/CLI (1) | regressions through shared engine/contract code | new constructs missing from the corpus | several minutes |
| Layout shift at most 0.1 | Console viewer pages | existing maintainer budget | version/build changes disturbing layout | unvisited page states | seconds after frontend build |
| Load time at most 2 seconds on loopback | Console viewer pages | existing maintainer budget | version/build changes delaying startup | production network conditions | seconds |
| Zero serious or critical axe violations | Console viewer pages | existing maintainer budget | inaccessible rendered controls | accessibility concerns axe cannot detect | seconds |
| Zero console errors, playground answer equal to stdio | Console viewer pages | existing maintainer budget and stdio parity (1) | swallowed query/render errors or broken browser-to-API path | unvisited interactions | seconds |

Use all four contract methods. A launch/version change also runs all four existing console
page budgets with a frontend built from this tree. The authorized independent checker can choose them without a new
approval. Reachability is established only by real starts after the serial build slot; a
present binary, frontend or configured service is not evidence of access.

Check and explain these captures:

1. `mixed-infra` plus `mixed-app`: one matched SQS queue, one unused provisioner, one
   unmanaged use. Across the whole workspace there is exactly one orphan of each sort,
   attributed to the repository holding that side. Neither absent-side marker appears.
2. `code-only`, `iac-only`: no orphan status line anywhere. Search and context mark the
   existing side with the issue's exact absent-side phrase. The full status remainder stays
   at base, including separate declared-resource writer/reader diagnostics.
3. `isolation`: same-spelled SQS/AMQP and S3/GCS names never combine their group presence;
   a declared resource kind `sqs` also stays separate from a channel broker `sqs`.
   Both sides remain one-sided, with markers and no orphan status lines. A manifest exposes
   only its listed repositories even with other inputs beside it on disk.
4. `patterns`: one S3 literal in two independent roots is used in each root, without
   ambiguity among provisioners; a bounded `archive-*` links to two literal names and a
   supported unused `reserve-*` is orphaned. Interior and too-short patterns stay outside
   provisioner sides, as at the exact base, and never create orphan provisioner rows. The
   unmatched literal uses remain orphans, separate from declared writer/reader diagnostics.
5. `refresh.json`: edit `parcel-idle` to `parcel-lost` without changing file size, query the
   same running stdio server, require the provisioner to appear and both status orphans to
   disappear, restore bytes, then require the original query and whole status to return.

For every base/head answer difference, retain raw captures and classify it line by line:
only the intentional orphan lines/markers and provisioner search visibility predicted by
#657 may differ. Exports must remain at base. Do not strip old writer/reader diagnostics to
make parity pass. The base is the exact SHA in the brief, never a nearby release.

Because the orphan hook reaches shared query code, run `prove.py run --repos all --http`,
then the console API recipe in `references/console.md` over that same run. A minor version
bump also reaches launch, so drive the viewer pages and existing budgets there. Preserve failed runs and cap product proof at
three rounds. Report infrastructure/socket failures as BLOCKED with their captures;
policy denial remains a failed step. Terminate only servers the run started, retain inputs,
captures and receipt, and confirm its PIDs stopped.

Could fool you: transport parity alone can agree on the same wrong marker; a generated
case alone misses real corpus regressions; passing a single orphan line misses extra noise
elsewhere in status. Inspect whole status and each independent issue-based expectation.
This probe does not establish OIDC grants; add an authenticated scope probe if that path is
changed and report the limit explicitly.

## Pinned infrastructure repositories (`scripts/infra.tsv`)

Pinned by commit on 2026-11-04 (the HEAD of each default branch that day); change a pin only with
the maintainer's OK. `prove.py` fetches and caches them like the corpus. `run --repos all` skips
them (most hold no code, so their exports have no symbol).

| Entry | Repository | Commit | License | For |
|---|---|---|---|---|
| `a7-probe` | `scripts/a7-probe/` (this skill) | the working tree | Apache-2.0 | stories 6 to 13, case by case |
| `a7-iac` | `scripts/a7-iac/` (this skill) | the working tree | Apache-2.0 | #649's Terraform shapes, case by case (its README lists them) |
| `a7-cfn` | `scripts/a7-cfn/` (this skill) | the working tree | Apache-2.0 | #653's templates: a SAM application, a plain template with a `NoEcho` parameter, a list and one with no default, `Fn::Sub` in its forms, `Fn::Join`, an import, a pseudo parameter, a JSON template, a file that is no template (its README lists them) |
| `a7-sls` | `scripts/a7-sls/` (this skill) | the working tree | Apache-2.0 | #654's services: `${self:...}`, `${opt:...}` and `${env:...}` variables, HTTP and queue events, a `resources` section, a `serverless.yml` with no `service` (its README lists them) |
| `a7-names` | `scripts/a7-names/` (this skill) | the working tree | Apache-2.0 | #650's names: locals, variables, var files, a `for_each` over a literal set and over a variable, a local module, `terraform.workspace`, a data source, a registry module, a conditional `count`, a sensitive variable (its README lists them) |
| `a7-noconfig` | `scripts/a7-noconfig/` (this skill) | the working tree | Apache-2.0 | a repository with no config: no config line, everything else as the base |
| `terraform-sqs` | terraform-aws-modules/terraform-aws-sqs | `a68b4d515f9ccc729fb8bf84c6760c6b91302ce0` | Apache-2.0 | Terraform on AWS: SQS queues, a registry module family, `for_each`, examples as root modules |
| `boutique` | GoogleCloudPlatform/microservices-demo | `c7cb6d5bfd6ea3aed768634ebbcb98cb7a523712` | Apache-2.0 | Kubernetes with Kustomize (`kustomize/`, `kubernetes-manifests/`), a Helm chart, Terraform, CI that builds the images, code in five languages |
| `crossplane-aws` | upbound/platform-ref-aws | `57fa3b0127c8ce263327f2d43b4e46d07fb90896` | Apache-2.0 | Crossplane: compositions, composite resource definitions, claims |
| `sam-rust` | aws-samples/serverless-rust-demo | `bd0164945a92ae1c05afc0b215f0eaa894e5ded9` | MIT-0 | a SAM application: a DynamoDB table, Lambda functions with API events, `CodeUri` next to the Rust code |
| `kube-prometheus` | prometheus-operator/kube-prometheus | `b6c2f62b287524652592e6e2f7ea0d3054cb8ac6` | Apache-2.0 | monitors as code: `PrometheusRule` objects (emitters). Its Alertmanager configuration sits in a `Secret`'s `stringData`, so it is redacted: the alert ticket pins a routing repository (OpsGenie, PagerDuty or Datadog in Terraform) beside it |

## `prove.py values`

```bash
python3 $S/prove.py values --base-bin "$RUN/bin/meada-base" --head-bin "$RUN/bin/meada-head" \
  --repos a7-probe,a7-noconfig,terraform-sqs,boutique,crossplane-aws,sam-rust,kube-prometheus,meada --run "$RUN/values" --http
```

Per entry it copies the entry, reads every config file with its own oracle (PyYAML's composer
and Python's `json`, never Meada's code or tables), indexes the copy with both binaries and
writes `values.md`:

1. **Oracle**: the files the walk visits (`git ls-files` under `.gitignore`, every `.ignore` and
   `.meadaignore` of the tree, hidden paths out but the six CI paths of ADR 0025, `node_modules`,
   `Pods`, `Carthage/Checkouts`, .NET `bin` and `obj` and symlinks out), each one a lockfile by
   name, unreadable (past 1 MiB, not UTF-8, not JSON for a `.json`, not YAML, past 1,000
   documents) or read into rows: key path, value, type, line, redacted, cut.
2. **`meada status`** (`LANG=C.UTF-8`): the head's config line equals the oracle's six counts, or
   is absent when the entry has no config file; every other line equals the base's.
3. **`meada export`**: the head's equals the base's, without `commit` and `dirty` (story 17).
4. **Value queries** over stdio, each `search` with `kind: value` and a budget of 200,000 so the
   whole answer comes back: per sampled file an exact value, its key path, its parent key path, a prefix and a substring of it; an
   upper-cased value; up to three redacted key paths; up to two cut key paths; up to four values
   that only a file left out holds (hidden, ignored, a lockfile); one query nothing holds. Each
   answer must equal the oracle's hits line by line: the same rows in the same order (exact,
   then prefix, then substring; then path, line, key path), the same repository, path, line, key
   path and shown value (`redacted`, a cut string with ` (cut)`). Then one value query again at
   `budget: 120` (the cut note must name the oracle's line count), the default search on up to
   three of the values (the base's answer when the base finds a symbol or contract, else the
   value hits) and an unknown kind (`values`), which must be refused with `value` in its list.
5. **`--http`**: the head serves the entry alone over HTTP and answers every query byte-equal to
   stdio.

`--sample N` sets the files sampled per entry (8). Exit 0: no difference, 1: differences, 2: a
run failed (BLOCKED).

## `prove.py iac`

```bash
python3 -m venv --system-site-packages "$RUN/venv" && "$RUN/venv/bin/pip" install -q python-hcl2==8.1.4 PyYAML==6.0.2
"$RUN/venv/bin/python" $S/prove.py iac --base-bin "$RUN/bin/meada-base" --head-bin "$RUN/bin/meada-head" \
  --repos a7-iac,a7-names,terraform-sqs --run "$RUN/iac" --http
```

Per entry it copies the entry and reads its `.tf` and `.tofu` files with `tf_oracle.py` (python-hcl2,
a Lark grammar of HCL, and its own rules: never Meada's code), which derives the nodes (address,
type, physical name, file, first and last line, root module) and the references, then writes
`iac.md`:

1. **`meada export` and `meada status`**: the head's equal the base's (no node is exported, no
   status line is added).
2. **Queries over stdio**, budget 200,000: per sampled node (`--sample`, 8: half with references
   both ways) `search` kind `iac` on its address, a five-character prefix and its physical name,
   `context` on `<address> (root <root>)` and `trace` down and up from it; `search` kind `iac` on
   three types; `context` on an address several roots hold (a `fits` list of that many); a query
   no node matches; the default search on three addresses (equal to the base's: nodes stay out);
   an unknown kind (`iacs`), refused with `iac` listed; `context` and `trace` up on three
   symbols of the code (`a7-iac` holds one named like a node), equal to the base's. Each `search` answer equals the oracle's
   hits line by line, each `context` its head, `type:`, `name:`, `root:`, `references:` and
   `referenced by:` lines (its `values:` lines are of the node's file; the values flow checks
   their text), each `trace` its depth-first lines and an `exact` footer (`none` alone).
3. **`--http`**: every query again over HTTP, byte-equal to stdio.

### IaC oracle conventions (where the ticket names no rule)

- A root is a directory no other directory calls through a `./` or `../` `source` that stays inside
  the repository; a directory nothing calls but a source climbing out is a root of its own.
- A traversal is found inside `${...}` by pattern; one naming no block of the instance is dropped,
  and `each`, `count`, `path`, `terraform` and `self` name nothing. A literal index that no
  instance has names every instance.
- `var`, `local` and module outputs are followed (at most 32 names), the first one kept as `via`;
  a module output that names nothing references the module's node.
- Ties sort by path, line, address and root (search) and path, line, address and via (context,
  trace), as `docs/reference/output-grammar.md` and #649's ADR section give them.
- A block's last line is found by matching braces outside strings, comments and heredocs.
- A physical name is the `name` of a resource or data block as `tf_names.py` evaluates it (#650);
  a module call has none (its `name` is one of its inputs).
- `tf_names.py` evaluates literals, templates, `var`, `local`, `each`, `count.index`, module
  outputs, lists, conditionals, `&&`, `||`, `==`, `!=`, `<`, `>`, `<=`, `>=`, `!` and the calls
  `upper`, `lower`, `format` (`%s`, `%d`), `join`, `toset`, `length` and `coalesce`. Any other form is unchecked: a name it
  cannot evaluate matches any name in `search` and its node is not sampled; a root with a `count`
  or `for_each` it cannot evaluate is left out (its lines dropped from both answers). The report
  counts both. A data source, a resource attribute, a registry module output,
  `terraform.workspace`, a sensitive or undeclared variable, and a call outside ADR 0025's list
  are `*`; a function over anything not known is `*` whole.
- A root's variable is the last value of `terraform.tfvars`, `terraform.tfvars.json`, then the
  `*.auto.tfvars` and `*.auto.tfvars.json` in name order, else its default, and is `overridable` (the value's line, or the variable block's first
  line); a local module's variable is the call's argument, else its default, never overridable.

### CloudFormation oracle conventions (#653)

- A template is a YAML or JSON file the walk visits (no hidden part, no symlink, within 1 MiB)
  whose document has a `Resources` mapping and `AWSTemplateFormatVersion` or the SAM transform
  (a string or a list holding it); a resource with a `Type` that has no `::` is no node.
- Lines: the node starts at its key's line (the outermost line of that name under `Resources`)
  and ends at its last value scalar's.
- `NoEcho` is true for `true`, `yes`, `on` and `y` in any case, plain or quoted (redacting too
  much is the safe direction). A parameter of a list type or of `AWS::SSM::Parameter::Value<...>`
  gives no name. `Fn::GetAtt` whose attribute is computed (`!Ref Param`) is a reference
  `via Fn::GetAtt`.
- References: `Ref`, `Fn::GetAtt` (both forms, `via Fn::GetAtt.<attribute>`), the `${Resource}` and
  `${Resource.Attribute}` variables of `Fn::Sub` that its map does not define, and `DependsOn`,
  anywhere in the resource; a name that is no resource of the template is dropped; at most 64
  per resource.
- A physical name is the property the oracle's own table gives per type (`NAMED`, the same short
  list the ticket's ADR section names), evaluated with `Ref`, `Fn::Sub`, `Fn::GetAtt` and
  `Fn::Join`; `Ref` of a resource is its name only where the table says so; any other function, a
  pseudo parameter, an import, a `NoEcho`, list or default-less parameter is `*`; a name over 1,024
  bytes, or nested over 32 deep, is `*`. A parameter's default is `overridable` at its `Default`'s
  line.

A difference on one of these is a question for the maintainer, BROKEN until answered.

## Stories 1 to 17 and what checks them

| Story | Checked by | Not covered |
|---|---|---|
| 1 value equal, prefix, substring | exact and prefix queries, every hit against the oracle | |
| 2 key paths | key path queries | |
| 3 file, key path, value, type, line | every hit line: path, line, key path, shown value; numbers and booleans by value | the type itself is not printed: only through the shown form (quoted or not) |
| 4 exact before prefix before substring, the budget | the order of every answer; the `budget: 120` query | |
| 5 default search unchanged, values as fallback | default search queries against the base and the oracle | |
| 6 every YAML and JSON file, no opt-in | the status counts and hits from every file | |
| 7 lockfiles and unreadable files counted | the status counts; `values.md` lists the oracle's unreadable files | the names are not printed by Meada (a follow-up of #639) |
| 8 `.meadaignore` | left-out queries; the read count | |
| 9 hidden CI paths | `a7-probe` holds one file per path; its read count | the image publication half is the image flow's |
| 10 every document, its index first | `a7-probe`'s `deploy/bundle.yaml` and `config/streams.yaml` | |
| 11 jq-style key paths with quoted keys | the oracle renders every key path on its own (`a7-probe`: `/`, `.`, `"`, `\`, a digit first, a space, non-ASCII) | |
| 12 cut with a mark | cut queries: the shown value is the first 1 KiB on a character boundary, then ` (cut)` | |
| 13 secret markers | redacted queries; a secret's value never matches by its text (`a7-probe` has a Secret, a Secret in a List, `data` and `stringData`) | Terraform `sensitive` and its tfvars values, CloudFormation `NoEcho` and `SecureString`: #639 left them to the IaC reading ticket (#640), which adds them to the oracle and to `a7-probe` |
| 14 the docs say what is indexed | the checker reads the user guide and capabilities pages at the head | no script |
| 15 grants | | needs server mode with OIDC: `tests/it/oidc.rs` `a_grant_scopes_the_config_values_of_a_search`, CI's `cloud` job |
| 16 fast substring | the seconds of each query in `values.md` | the spec names no number: the checker judges and says so |
| 17 values out of the export | the export against the base | |

## Oracle conventions (where the spec names no rule)

- Types follow the YAML 1.2 core schema on plain scalars (`yes` is a string, `True` a boolean,
  `0x1F` a number); quoted and block scalars are strings; a custom tag (`!Ref`) keeps its text.
- A scalar reached through an alias sits at the alias's line, where its key path is.
- A block scalar (`|`, `>`) sits at its first line of text, one past the indicator.
- A number compares by value: Meada keeps it as parsed (`1e3` shows `1000.0`); an integer compares as an integer.
- Ties sort by path, line and key path (ADR 0025 says "rank, tie-break and line"; #639's docs name the tie-break).
- A YAML merge key is a key like any other (`merged["<<"].host`): YAML 1.2 has no merge key.
- A `.json` file must parse as JSON first; its tabs read as spaces for the line numbers.

A difference on one of these is a question for the maintainer, BROKEN until answered.

## Could fool you

- The oracle samples one value per file (`--sample`): a bug in a file shape nobody sampled passes. `a7-probe` holds every shape stories 6 to 13 name; a change about another shape adds it there (and to its README) before the run.
- A base built before #639 knows no kind `value`: its value answers are refusals and are not compared. Only status, export and the default search answer to it.
- A default search sampled only on config values never asks story 5's "as before when a symbol matches": `values` also asks three symbol names, and `a7-probe` holds a value equal to a symbol (audit 2026-11-04, M6, caught before only by luck of the corpus).
- A difference on one of the oracle's conventions is a question for the maintainer, never a fix to the oracle.

- `prove.py iac`'s oracle encodes #649's rules as its author read them, the same author as the code
  (#649 wrote both): a rule misread in both agrees with itself. The checker reads the ticket's
  criteria against `iac.md`'s samples, and `/audit-prove-it` plants bugs to test the oracle.
- `iac` samples 8 nodes per entry: a reference shape no sampled node holds passes. `a7-iac` holds
  every shape #649 names; a change about another adds it there (and to its README).

## Timings measured on 2026-11-04 (debug builds, 4 cores)

`values` over `a7-probe`, `terraform-sqs`, `crossplane-aws`, `sam-rust`, `boutique` and
`kube-prometheus` with `--http`: about 1 min. `meada` alone: about 3.5 min (92,777 values; the
oracle's PyYAML read is most of it). The slowest value query: 0.9 s on `meada`. `iac` over `a7-iac`
and `terraform-sqs` with `--http`: about 20 s; the slowest query 0.02 s.

## Terraform provisioning (#656)

Run `python3 $S/provisions.py --head-bin "$RUN/bin/meada-head" --run "$RUN/provisions"`.
The script creates neutral sibling repositories, never reads the feature's type table or test
fixtures, and drives the compiled binary: eight provisioning links, every use at both roots,
levels and overridable reasons, four exported nodes and provisioners, six context/trace queries
through stdio and HTTP, and a same-size variable edit that removes two links then restores them.
Its answer key is the supplied #656 ticket and ADR 0026, not its own output. The checker also
reads the source behind each result and probes ARN/URL names, two roots sharing a local module,
unknown types, unresolved names and the token budget. Existing code-only answers still compare
against the base binary with `prove.py run --http` on the corpus selected by Scope.

Export goldens use `meada_core::export::Export::write`, after clearing only fixture metadata
and processes, rather than a generic JSON writer. The format-1 public contract includes canonical
field order: JSON-value equality alone cannot prove its byte round trip. Run the public core
export test named in Scope whenever an export golden changes.

The earlier IaC oracle's assertion that every export equals base predates #656. For its Terraform
corpus, inspect the complete export diff: only `iac_nodes`, `provisioners` and `provisions` may be
added, each justified by source. Preserve the original oracle report and its failed equality;
report this expected new flow separately from unchanged fields. Do not turn a failed oracle
check green by deleting the added arrays without verifying every row against the input.

This script's author also wrote the implementation. A fresh checker is required; the script is
not an independent answer key. The script checks concrete and pattern names. The console graph
additionally loads this generated
workspace: its contracts show both root labels and its provisioning table shows every link, level
and reason. Orphan status behavior remains pending.
