Probe `a7-tf-set` of prove-meada (#740): a Terraform variable declared `type = set(...)`, then
retyped or given a default by override files. One root per row of the maintainer's table in
#740 (the comment of 2026-11-09, "The rows, run in the tools"), each with the issue body's
resource:

```hcl
resource "aws_sqs_queue" "q" {
  for_each = { for k, v in var.s : k => v }
  name     = "queue-${each.key}-${each.value}"
}
```

The answer key is `terraform-names.tsv`: per root, the names Terraform v1.17.1 creates, or
`rejected` where Terraform refuses the configuration. The maintainer's column was run in
Terraform's `console` on `[for k, v in var.s : "${k}-${v}"]`, the same key and value this
resource joins, so each name is `queue-` and the column's value. Nothing in it comes from Meada.

| Root | Declared, default | Override | Terraform v1.17.1 |
|---|---|---|---|
| `no-override` | `set(string)`, `["b", "a", "a"]` | none | `queue-a-a`, `queue-b-b` |
| `tolist` | the same, read by `tolist(var.s)` | none | `queue-0-a`, `queue-1-b` |
| `list` | `set(string)`, `["b", "a", "a"]` | `type = list(string)` | `queue-0-a`, `queue-1-b` |
| `list-pair` | `set(string)`, `["b", "a"]` | `type = list(string)` | `queue-0-a`, `queue-1-b` |
| `any` | `set(string)`, `["b", "a", "a"]` | `type = any` | `queue-a-a`, `queue-b-b` |
| `set` | `set(string)`, `["b", "a", "a"]` | `type = set(string)` | `queue-a-a`, `queue-b-b` |
| `tuple` | `set(string)`, `["b", "a", "a"]` | `type = tuple([string, string])` | rejected |
| `tuple-pair` | `set(string)`, `["b", "a"]` | `type = tuple([string, string])` | rejected |
| `list-own-default` | `set(string)`, `["b", "a", "a"]` | `type = list(string)`, `default = ["b", "a", "a"]` | `queue-0-b`, `queue-1-a`, `queue-2-a` |
| `default-only` | `set(string)`, `["x"]` | `default = ["b", "a", "a"]` | `queue-a-a`, `queue-b-b` |
| `default-then-list` | `set(string)`, `["x"]` | `a_override.tf`: the default; `b_override.tf`: `type = list(string)` | `queue-0-a`, `queue-1-b` |
| `declared-list` | `list(string)`, `["b", "a", "a"]` | `type = set(string)` | `queue-a-a`, `queue-b-b` |

The table's child-module row is left out: which root reports a child module's resource is not
what #740 decides.

`prove.py iac` cannot evaluate these names (`tf_names.py` reads no override file and no `for`),
so every root is left out of its oracle comparison. It reads `terraform-names.tsv` instead and asks
`search` kind `iac` for each type: an exact name (`name "..."`) must be Terraform's, and a root
Terraform rejects must have none. A root with no exact name is absent, not wrong (certain or
absent); the report names those roots. Meada leaves a `for` key over a variable declared
`type = set(...)` unresolved (`docs/agents/ceilings/`, Terraform and provisioning), so the
set-keyed rows are expected absent. Whether an absent row meets #740's acceptance ("each row
answers the Terraform column at exact") is the judge's question, never the script's. OpenTofu
1.13 and later give other names for several rows; Meada follows Terraform (#740).
