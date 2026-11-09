# a7-iac

The IaC nodes probe of prove-meada (#649): Terraform shapes `terraform-sqs` does not hold, each
read by `tf_oracle.py` on its own. `live/prod` and `live/dev` are root modules; `modules/bucket`
is called twice by `prod` and once by `dev`, and calls `modules/topic` with `count`; `stray/`
is a root of its own (only a source that climbs out of the repository names it).

- `count` and `for_each` over a map and over `toset`, a non-literal `for_each`, `count = 0`
- references through `var`, `local` (a chain, and a cycle), module outputs (one that names
  nothing), a splat, a literal index, `depends_on`, a data block with `count`
- a registry module, a Git module, a `source` that climbs out of the repository
- a `.tofu` file that shadows a `.tf` file of its name, and a file that does not parse
- `zz_code.py`: a Python method `aws_sns_topic.alerts`, named like a node: its `context` and
  `trace` must stay the base's
