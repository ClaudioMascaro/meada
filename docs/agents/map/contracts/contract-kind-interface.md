# Contract kind interface

- `contract.rs`: `ContractKind` trait, the whole interface of a kind: `prefix`, `extract` and
  `link` are required, every other question (see [New contract kind](../overview/seams-and-what-extends-them.md))
  has a default body. The engine fixes the phase order (every `declare`, every `workspace_step`,
  `link`, the decisions replay, every `after_links`) and asks the kinds in `KINDS` order; a new
  kind appends to the list. A side is unresolved when its contract is absent (an unresolved
  consumer) or when `unresolved` says so of the identifier (an unresolved side). The seam's free
  functions (`kind_of`, `unresolved`, `shown`, `typed`, `label`, `parts`, `place`, `serves`,
  `entry`, `searched`, `explain_inferred`) are what the rest of the crate calls. The
  `explain_inferred` display policy defaults to false and receives the consumer's repository.
  HTTP enables it only when the recorded `env:` base selects a peer declared by `base_url_env`,
  using the same `Config::peers_reading` lookup as its links after stripping a proxy endpoint
  suffix. It changes no links. A kind's helpers are private to its
  file; the few that stay `pub` are named under their kind (`http::contract`,
  `normalize_path` and `PARAM` for the config and the documents, both `meada_core::http`'s,
  re-exported; `channel::contract` for
  the documents; the `package` readers the index writer calls). `KINDS`, `Level`, `Link`, `Names`, `Role`. A `Found` carries its `role` (`Provider`,
  `Consumer`, `Binding`, `Publish` or `Import`),
  stored in `contracts.role`; the site does not imply it (a channel producer is a `Site::Call`, a channel
  consumer a `Site::Def` or `Site::Inline`). A `Site::Def` name is folded through `Names::scope`
  in the index: unnamed inline segments drop, and an empty result is the file's top level.
