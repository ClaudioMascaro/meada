# Configuration values

- `config_values.rs` (#639, ADR 0025): `VALUE_MAX` (1 KiB: a value is stored cut at the last
  character boundary within it, marked `(cut)` in the answers, and matched as stored);
  `DOCUMENTS_MAX` (1,000 documents per file: a stream of more is unreadable, since a manifest
  bundle holds tens and past a thousand it is generated output); `STORED_EXPANSION` (the bytes of the
  key paths and values one file stores, 4 times its size plus `STORED_SLACK`, 4 KiB, the hostile
  probes' bound:
  an indented file stores about twice its size, but a minified file of tiny scalars, such as a
  long array of numbers, and long keys nested deep are unreadable). An unreadable file and a
  lockfile are counted in `meada status`, not named. Search kind `value` finds exact and prefix
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
  counted). The parser's own limits (`hcl-edit` 0.10's `MAX_OPERATOR_DEPTH`, 128 chained
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
