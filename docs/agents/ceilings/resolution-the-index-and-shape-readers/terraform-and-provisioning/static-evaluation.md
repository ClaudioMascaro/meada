# Static evaluation

- `iac.rs` static evaluation (#650, ADR 0025): `EVAL_VALUE_MAX` (64 KiB of one evaluated value,
  as `iac::size` counts it; past it, the value is unresolved, and a template past it is `*`
  whole), `EVAL_COST_MAX` (1 MiB of evaluation steps one expression could take, `iac::cost`'s
  upper bound, #714: a step is a byte made or, each time it runs in a `for`, one of the steps that
  make none: `TEMPLATE_PART_COST` (8) per part of a template (a literal, an interpolation or a
  directive), whatever bytes it makes (`iac::Exact` makes a template the call of its parts, so
  `hcl-rs` never parses one again, #763), and a byte of the root object a traversal reads (`hcl-rs`
  clones `var`, `local`, `each` or `module` whole to read `var.x[0]`); a part is charged in a `for`
  expression and in the body of a `%{ for }` directive, so a `%{ for }` of one interpolation over
  140,000 literal elements, exact before, is refused (a body of no part is bounded by its
  collection, whose elements each evaluation charges); outside both, a part is charged nothing; a
  `for` collection counts its own elements through any root, a literal list or object as many as it
  lists at its costliest element's cost, and its elements once, read or not (#716), a conditional
  its larger branch; `join`, `replace` and `formatlist` are charged their arguments and the output
  they can make (#764), never
  more than the square of their arguments they were charged before, a string argument at what it
  shows (a literal its length, anything else three times its cost: a
  number shown takes up to 24 bytes, where 8 are counted): `join` its lists times the separator
  plus three, `replace` its string times the replacement plus one, `formatlist` a row per
  element of its shortest literal or bound list (else as many rows as its costliest argument of
  no known kind), each row the format, `WIDTH_MAX` per argument and six bytes per byte of what
  it formats (`%q` escapes), a list's elements once and any other argument in each row; with an
  expanded last argument (`f(x...)`) the three are still charged that square, a list literal
  counts each element a byte at least (a template of no text costs nothing), so its cost
  bounds how many elements it holds, and a call's charge counts again in the call around it, so by a reading of `cost`
  eight nested one-element `join`s are refused where Terraform names `a`; past `EVAL_COST_MAX`
  `hcl-rs` does not evaluate the expression, only its
  template parts resolve; the implementer measured about 0.2 s for an expression at the ceiling
  (15abf1c0, debug)) and `EVAL_WORK_MAX` (64 MiB of cost per root module; past it every value left is
  unresolved, but for a literal name that fits `EVAL_VALUE_MAX`. A refusal spends an upper
  bound of the bytes it went through to decide, never more than `EVAL_COST_MAX`: its own text,
  and the size of each root (`var`, `local`, `module`, `each`, `count`) times two plus twice
  its reads of that root (a resource it names is none, and a read of `count.index` counts the
  `count` root alone, #765), so 9,999 refusals over a 21,000-element variable stop
  evaluating after 89 and take 2.3 s in process (1bdcc75d; a cold index of that root took
  4.30 s at b045c759 against 4.17 s on main 05585ce4, then 4.46 s at ae8139de against 4.40 s
  at b045c759, all debug), and a long expression refused in each instance of a `count` stops
  once its text has spent the work. The steps it charges bound a root of expressions near the
  ceiling: 80 resources took 16.6 s (the implementer's, 1bdcc75d, debug). It does not charge
  what no step counts: a template outside every `for` that reads `each` or `count` is evaluated
  again in each instance, its parts with no parse since #763, and a template past `EVAL_VALUE_MAX`
  is evaluated whole before that ceiling refuses it (10,000 interpolations and `count.index`: 0.60 s
  at `count = 1`, 1.25 s at 10 and 8.45 s at 100 at ca6794c8, where the parse took 0.89 s, 3.75 s
  and 33.0 s at 60db9e4d; one 1 MiB template of 130,000 interpolations, `*`, 6.4 s at ca6794c8 and
  9.4 s at 60db9e4d; the implementer's, debug); the work is per root, not per repository (20 roots
  of 80 resources near the ceiling took 234.75 s, measured by a black-box verifier on a debug
  build); and the estimate itself is charged nothing, though it sizes each bound value it reads once
  per expression (#765), and a known name that reads neither `each` nor `count` is evaluated once
  for every instance of its block (20,000 reads of a 21,000-element variable in a `for` over an
  empty collection: 1.31 s at `count = 1` and 1.50 s at 10 at 7f413fdb, where each instance paid the
  estimate again: 2.74 s and 16.59 s at b8778732, debug, one run each); a name that is not known
  (a pattern or unresolved), or that reads `each` or `count`, and any other attribute read per
  instance, is evaluated again in each instance, at a cost linear in its text that only a
  refusal charges (about 0.33 s an instance for that name at b8778732, measured with each read
  `var.big[0]`, whose estimate was already cheap: 1.75 s at `count = 1`, 4.72 s at 10);
  converting an object literal for `hcl-rs` also spends the square of its keys, so a wide local
  evaluated in many module instances stops there; their nodes stay, named `*`). A var file's
  values are bounded the same way, each file with an `EVAL_WORK_MAX` of its own: a value past
  `EVAL_COST_MAX`, that work or `EVAL_VALUE_MAX` is given but unresolved (the default does not
  stand in for it), and one that is no constant is left out, as Terraform refuses it. A var file
  that does not read (past `MAX_BYTES`, with HCL objects past `OBJECT_WORK_MAX`, or broken) may
  give any variable: each is unresolved unless a file Terraform loads after it gives the value;
  `WIDTH_MAX` (`iac/functions.rs`: a `format` width of 256; past it the call is unresolved).
  `iac::Exact` puts integer arithmetic's exact value in place first; a result outside
  `i64::MIN..=u64::MAX`, a division with a remainder (`7 / 2`, 3.5 in Terraform) and arithmetic on a
  fraction are unresolved (in a var file, given but unresolved, so the default does not stand
  in); an operation on a `for` variable becomes a call of `meada_exact`, the same arithmetic at
  evaluation time (declared alone in a var file, which calls no function). A number
  `hcl-rs` would print otherwise than Terraform (past `i64::MIN..=u64::MAX`, more digits than a
  64-bit float holds, or a float it writes with an exponent, past 1e16 or below 1e-5) is
  unresolved too, as a literal or a `tonumber`, and so is a JSON number written with a fraction or
  an exponent or past that range, even `1.0`, in a `.tf.json` or a var file (`serde_json` keeps no
  digits). A negative zero (`-0`, `-(0)`, `0 * -1`), `-0` in Terraform, is unresolved even where
  only its value counts, since `hcl-rs` numbers hold no negative zero: an index, a comparison, and
  a `count`, which gives one node as any unresolved `count` does, where Terraform makes none. `cost`
  bounds a `for` by the size of what it iterates, so a `for` over a value it cannot size (a
  function's result, another `for`) is costed as its square and may stay unresolved below the real ceiling.
  The type table selects `bucket` for S3 and `name` for SQS; unlisted types still read
  `name` (no `function_name` or `name_prefix`). A resource's attribute is never evaluated, even an
  argument its configuration sets, so an output of `aws_sqs_queue.this.name` is `*`. A `count`
  or `for_each` that reads a module output does not resolve (module instances are made before
  outputs are read). Only the var files Terraform loads by itself give values
  (`terraform.tfvars`, `terraform.tfvars.json`, `*.auto.tfvars`, `*.auto.tfvars.json`): a
  `-var-file` is not read, and a JSON var file's value line is the first occurrence of its key.
  A value cut at `EVAL_DEPTH_MAX` or around a cycle is memoized for its instance, so a shallower
  reader of the same local gets the cut value. A `for_each` over a list of strings expands unless
  Meada knows it is a tuple, which Terraform and OpenTofu refuse (#769): one written out (`[...]`,
  `[for ...]`), or a variable read whole that holds one (a var file's value or a default under
  `any` or no type, a declared tuple type, a module argument that is a tuple in its caller). Any
  other list of strings expands (`toset` gives a list too), so a list or a tuple both tools
  refuse, from a declared `list(...)`, a local or a function (`concat`, `keys`, `tolist`), names
  its elements at `exact`. `tolist`, `toset`
  and `tomap` convert their elements to one type as cty unifies them (scalars with a string
  become strings, objects of one set of attributes attribute by attribute and others to a map,
  tuples of one length element by element and others to a list, a number with a bool refused). A
  list value is read as a tuple, a set as a tuple and a map as an object, so a `tolist`, `toset`
  or `tomap` result beside a literal inside another conversion may be exact where Terraform
  converts more to strings, keeps the set's element type or refuses the call. A `for` binding a
  key over a `toset(...)` call, bare, in parentheses or a conditional's branch, is unresolved
  (Terraform's key is the element, or the index when the other branch is a list: a name whose
  conditional has a set in one branch is unresolved the same way, though Terraform unifies both
  branches to a list when the other is one), and so is one
  over a local or a variable that is such a call, another such name or a variable declared
  `type = set(...)`, but not one declared with another type (`list(...)`, `object(...)`, a
  primitive), which converts the value; over a set reached through an attribute, an index, a module output or
  another call, its key is the index (#717). A
  null beside a collection is unresolved, but for a `tomap` whose other values share one type.
  `toset` orders as cty
  does (strings bytewise, numbers by value, `false` before `true`, a null last); a set of
  collections, in cty's hash order, is unresolved. An object walks its keys in lexical order, as Terraform
  does: a value a name binds, a literal whose keys are written as names or strings, a `merge`,
  anything `jsonencode` writes. A `for` whose collection expression contains, anywhere, an object
  whose keys are known only once evaluated (a computed key, a comprehension) is unresolved, even
  where the order does not show (a `length` of the `for`, or a `values`, `lookup` or `jsonencode`
  of that object inside the collection), and a template `%{ for }` directive is not evaluated (a
  `*`). A file's own call of `meada_exact` is no function.
  A function over an unresolved argument is `*` whole, even
  `format` or `join`, which keep no literal part. A literal
  `*` in a name reads as a wildcard. A child module's variable default its call leaves is not
  overridable. Names through a cycle of locals are partly resolved, the member met again a `*`.
