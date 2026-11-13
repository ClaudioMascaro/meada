# Configuration values

- `config_values.rs` (#639, ADR 0025): `VALUE_MAX` (1 KiB: a value is stored cut at the last
  character boundary within it, marked `(cut)` in the answers, and matched as stored);
  `DOCUMENTS_MAX` (1,000 documents per file: a stream of more is unreadable, since a manifest
  bundle holds tens and past a thousand it is generated output); `STORED_EXPANSION` (the bytes of the
  key paths and values one file stores, 4 times its size plus `STORED_SLACK`, 4 KiB, the hostile
  probes' bound:
  an indented file stores about twice its size, but a minified file of tiny scalars, such as a
  long array of numbers, and long keys nested deep are unreadable). An unreadable file and a
  lockfile are counted in `meada status`, not named. `VALUES_MAX` (#817: 1,000 config values
  per YAML or JSON file that only this reader takes, all its documents together, redacted ones
  counted, since configuration a contract or a rule reads holds tens to hundreds of values and a
  data dump thousands): past it the file is data and keeps none, and `meada status` counts it
  apart and names the first `DATA_NAMED` (10) data files in path order, the rest as a count.
  Terraform and OpenTofu files in every form, and a file a reader of a kind takes as its own
  (`config_values::kind_file`: a document, a config file a language pack reads or includes, a
  Compose file, a GitHub workflow or Skaffold file, a pipeline or an action by its place and
  name (`iac::actions::is_file`: also an `action.yml` that holds no `runs`), a package
  manifest, a kustomization or a
  file the Kubernetes reader takes an object from, a CloudFormation or SAM template or
  Serverless service, a file the alert reader takes a rule, a route or a receiver from), keep
  every value. The kinds are asked only past the ceiling. A file that a `[serve]` listing or a
  language pack starts or stops taking is read again (the index keeps the files they took,
  `taken`), so a refreshed index holds what a new one would. What is given up: a large generic values file, such as a Helm chart's
  `values.yaml` past 1,000 values, keeps none, so it gives no `value` hit, no declared site and
  no config use, and `.meadaignore` cannot bring it back (the image reader still takes its
  images from the file's bytes). Search kind `value` finds exact and prefix
  matches through the B-trees on the lowercased key path and value, and substrings through the
  FTS5 trigram table `config_search` (#647); a query of under three characters (or holding a NUL)
  scans every row of every repository, and the trigram table stores about four times the text of
  the key paths and values (no marker on a `const`: the three-character floor is the
  tokenizer's). Redaction covers a Kubernetes `Secret` at a document's root or as an item of a
  list's `items`; a `Secret` with no `apiVersion`, or nested anywhere else (a Helm value, a
  Crossplane composition's base), keeps its values (the other secret markers of ADR 0025 belong
  to the IaC formats, #640). The hidden CI paths are read at the repository root only, and only
  their config files: YAML and JSON, and since #640 Terraform too, so a Terraform root kept under
  `.github/` (boutique's `.github/terraform`) gives values and nodes, and its `sensitive`
  variables redact what var files anywhere give them (#709 kept it for that redaction).
  Terraform and OpenTofu (#640, `config_values/hcl.rs`): `NESTING_MAX` (64 brackets open inside
  each other in one file, counted by a scan that includes the ones inside strings, so a file
  with many unbalanced brackets in strings can be unreadable; past it the file is unreadable and
  counted) and `OBJECT_WORK_MAX` (2^24 key comparisons per file: `hcl-edit` and `hcl-rs` check
  each key of an object literal against the keys before it, so a file's objects cost the sum of
  their keys squared, counted before parsing by a scan that skips strings, heredocs, comments
  and block bodies (a `{` after an identifier or a quoted label, the `in` or `if` of a `for`
  aside). The count is only ever too high: every `=` or `:` that is no operator counts in the
  object around it, a conditional's `:` too, and a block typed or labelled `in` or `if` counts
  as an object. A template directive's body, from its `%{ if }`, `%{ for }` or `%{ else }` to
  the directive that ends it, is text past a quote or a heredoc's end line, as `hcl-edit` reads
  it. Past it, as one object of 4,097 keys is, the file is unreadable and counted, and
  Terraform evaluation and the alert reader skip it too; so is a file with a heredoc whose
  first line starts with its identifier (after spaces or tabs, no identifier character after
  it): `hcl-edit` ends it on that line only when no later line could, a choice the scan cannot
  follow, and reads other values than Terraform when one could). The parser's own limits (`hcl-edit` 0.10's `MAX_OPERATOR_DEPTH`, 128 chained
  operators, and `MAX_NESTING_DEPTH`, 64 levels of nested expressions, templates and blocks) make
  a file past them unreadable, counted by `meada status`; `PARSER_OPERATOR_DEPTH` and `PARSER_NESTING_DEPTH`
  in `hcl.rs`' tests mirror them, and a test feeds a file at and past each. An `expression` or
  `template` is shown on one line, its line breaks written `\n` (`config_values::one_line`).
  The value length and stored budget are the YAML ones. A `.tfvars` value is
  redacted by variable name across the repository (a sensitive name redacts it in every
  var-file, whichever module it goes to), and wholly when a Terraform file at or above its
  directory is unreadable (`config_values::ANY_VARIABLE`); a variable marked sensitive by an
  expression keeps its values, and `sensitive` is read as the literal `true` of a top-level
  `variable`, including JSON arrays at label or body levels. Pending JSON redaction paths
  carry no values and charge the stored budget before they are copied. Completed marker paths
  have a separate budget of the same size, charged before rendering; a later repeated section
  cannot erase a marker. Past either budget the file is unreadable and keeps no value rows.
  An `aws_ssm_parameter` resource with literal
  `type = "SecureString"` redacts its `value`, regardless of attribute order, in HCL and the
  object and array forms of Terraform JSON. When a `type` is present, only the explicit public
  types `String` and `StringList` permit its value; an unresolved type stays redacted. A `//` comment
  property of a Terraform JSON file is a value, and a block written as an array of one is
  indexed `[0]` there. A number is stored as
  written. A block repeated in one body is indexed from its second occurrence on, so adding a
  second block renames the key path of the first. A `.tf` file a `.tofu` one shadows is neither
  read nor counted. The old value of a variable made sensitive later stays in SQLite's free
  pages until they are reused (`secure_delete` is off).
- `iac/config_uses.rs` (#828, ADR 0028): the key-name rule skips the files the CI reader owns
  (a pipeline, an action), by the reader's own file node and never by name: a file under
  `.github/workflows/` with no `jobs`, an `action.yml` with no `runs`, and one the reader could
  not read keep the rule. In an owned file only a structured identifier is a use. Its holder is
  the nearest CI node whose address starts the key path: a CI step, else its CI job, else the
  file node. The holder is the `site` of `uses` and the `side` of `provisions` in `meada
  export` alone: `context`, `search` and `trace` place the use by its key as before, so
  `trace` from the key does not climb to the callers of its pipeline. A `${{ }}` expression is
  closed at the first `}}` (one that holds `}}` in a string is cut there) and one left open
  leaves the value as written. Only `env.<name>` alone resolves, the name of letters, digits
  and `_` in the case written: `env['X']`, another case, a function or an operator are
  unresolved. The literal is a string of `<node>.env.<name>` or of the pipeline's `env.<name>`,
  the nearest above the key and never the map the key is written in; a number or a boolean, a
  redacted or cut value and one that holds an expression or a `*` are no literal, and a
  composite action reads only the `env` of its own CI steps. An unresolved part is `*`: in the
  name it makes a pattern under ADR 0026's limits, as the whole name no use and no open item
  (the config value alone names the variable), outside the name (an ARN's account, the object
  key after a bucket) nothing. A value with a written `*` is not evaluated. A role ARN gives no
  use because IAM is in no row of the type table, in any file: nothing reads `with` apart. The
  image builder forms keep the repository as their site until #831.
