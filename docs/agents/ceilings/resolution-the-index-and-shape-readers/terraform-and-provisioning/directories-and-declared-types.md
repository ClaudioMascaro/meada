# Directories and declared types

- Meada reads a directory as Terraform does, and OpenTofu 1.13 and later differ (#740; the
  maintainer chose Terraform). Terraform converts a variable's default to the type its own block
  declares and again to the type an override file gives; OpenTofu 1.13+ converts it once, to the
  final type. Meada follows Terraform in each row below and OpenTofu 1.13+ differs in each,
  under `type = set(string)` with a default `["b", "a", "a"]` (in the pair of override files,
  over a declared default `["x"]`): an override `type = list(string)` (Terraform `0-a`, `1-b`;
  OpenTofu `0-b`, `1-a`, `2-a`), `type = any` (the set, `a-a`, `b-b`; OpenTofu index keys),
  `type = tuple([string, string])` over `["b", "a"]` (Terraform rejects it, so Meada gives no
  exact name; OpenTofu `0-b`, `1-a`), an `a_override.tf` giving the default and a
  `b_override.tf` retyping it, a child module called without the argument with the override in
  its directory, and a `null` default retyped `tuple([string, string, string])` with a var file's
  `["b", "a", "a"]` (Terraform rejects the default, so Meada gives no exact name; OpenTofu `0-b`,
  `1-a`, `2-a`). The same default with no override, a restated `set(string)`, an override
  giving `type` and `default` together and a var file's value name the same in both. OpenTofu
  1.13.9 was run; that 1.12 and earlier behave as Terraform is read from the source, not run.
  The exception covers a variable all of whose blocks Terraform reads (`.tf`, `.tf.json`): it
  follows Terraform, whatever the override. A variable with a block in a file only OpenTofu
  reads (`.tofu`, `.tofu.json`), its declaring block or an override file's, is one Terraform
  does not read whole, and OpenTofu 1.8 to 1.12 (read from the source, not run) convert its
  default as Terraform does where 1.13+ converts once, so no value is certain for it: when an
  override retypes it (in any of `override.tf`, `*_override.tf`, `override.tf.json`,
  `override.tofu`, `*_override.tofu`, `override.tofu.json`) the default is unresolved where the
  two conversions hold other elements (a set's compared once converted, so `[10, 9]` and
  `["10", "9"]` are the same under `set(string)`; a list's as a name spells them, so
  `["010", "9"]` declared `list(number)` and retyped `list(string)` or `any`, `10` converted
  twice and `010` once, is unresolved) or another kind of collection, and exact where they
  agree, with OpenTofu 1.13's value (no override, an override restating the set, an override
  with its own default of the final type, a var file's value, and a default its types convert
  alike once and twice). `["10", "9"]` declared `list(number)` in a `.tofu` file and retyped
  `any` holds OpenTofu 1.13's strings, so a read that tells `"10"` from `10` names OpenTofu's
  instances, not the ones Terraform names with that override in a `.tf` file. A declaration
  Terraform reads, followed by a block in a file only OpenTofu reads, has a second reading,
  Terraform's without that file (#767): the variable is unresolved where the two give it another
  value or another type, even where both tools name alike (`main.tf` with the set above and an
  `override.tofu` giving `default = ["c"]`: Terraform `0-a` and `1-b`, OpenTofu `0-c`;
  `["10", "9", "10"]` declared `list(string)` and retyped `list(number)`: both `0-10`, `1-9`,
  `2-10`), and exact where they give one (an override restating the default or the type, a
  `.tofu` override's type and then a `.tf` one's, a var file's value or a module argument under
  one type). Types are compared as written, whitespace aside, and no `type` is `any` (`"010"`
  declared `string` and retyped `number`: Terraform `f-010`, OpenTofu `f-10`). Where OpenTofu
  refuses the directory, Terraform's reading is the variable's, and Meada tells one refusal: a
  string no number spells in a default a `.tofu` block retypes `list(number)` or `set(number)`
  (`["z"]` declared `list(string)` names `0-z`, and Terraform's instances of a var file's value). A
  default Terraform refuses under a type only OpenTofu reads names nothing exactly. A local
  overridden in a `.tofu` file keeps OpenTofu's value (`"b"` in `override.tofu` over `"a"` names
  `f-b`, Terraform `f-a`): only variables are compared. A variable declared in a file Terraform
  reads and again in a file only OpenTofu reads, which OpenTofu refuses ("Duplicate variable
  declaration"), is the `.tf` block with the override files Terraform reads, in either order of the
  files, though a `sensitive` the `.tofu` block gives still holds (#768). A module is judged by its
  own files, whatever calls it: a module of `.tf` files called only from a root written in `.tofu`
  follows Terraform though Terraform cannot read that root, and OpenTofu's answer differs.
- A variable's declared type converts what it receives (a default, a var file's value, a module
  argument) where Meada can make the conversion with certainty: a `set(T)` converts its elements,
  sorts and deduplicates them, and a `list(T)` of `string`, `number` or `bool` converts its
  elements in their order (`["010", "1.50"]` under `list(number)` names `10` and `1.5`, #770).
  An element Meada cannot convert with certainty leaves the whole value unresolved, `-0` under
  `number` among them: Terraform keeps a zero's sign and Meada's numbers do not. Meada converts
  no other type: a primitive, a `list(any)`, a list of collections or objects, a tuple, a map
  and an object keep the value as written, and a name read from one is exact even where
  Terraform converts an element and spells it otherwise (`"010"` given to `type = number`
  names `010`, where Terraform names `10`). A default its types refuse leaves the value of every
  source unresolved, as Terraform and OpenTofu refuse the configuration whatever gives the value
  (#769), and so does a default Meada cannot convert with certainty, though both tools take it:
  `["-0"]` under `list(number)`, given `["1", "2"]` by a var file or a module argument, names
  nothing exactly where both name `0-1` and `1-2`. A variable with a block in a file only
  OpenTofu reads is not checked this way: declared in a file Terraform reads, it is compared
  with Terraform's reading (#767, above).
