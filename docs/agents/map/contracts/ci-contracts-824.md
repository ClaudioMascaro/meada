# CI contracts (#824)

`ci.rs` owns `ci::<repository identity>/<path>` (ADR 0028): a reusable pipeline as provider, each
CI job that calls it as consumer. It extracts nothing at index time: both sides are read at
query time, in `workspace_step`, from the CI nodes `iac/actions.rs` stores, because a call is a
consumer only when the repository it names is one of the workspace, which the index of one
repository cannot know. The provider sits on the file node `.` whose `iac::actions::Facts` says
`reusable` (`on` holds `workflow_call` as a string, a list item or a map key). The consumer sits
on the `job` node whose `uses` reads `<owner>/<repo>/.github/workflows/<file>@<ref>`
(`Call::read`). A `uses` writes no host, so the host is the one of the consumer's identity, with
owner and repository lower-cased as an identity is and the path as written; a repository
without `origin` (`local/...`) names no peer. A call whose repository is not in the workspace
holds no side at all: nothing lists it as an orphan or an open item. One whose repository is
there and whose path names no provider is a consumer with no link.

`link` joins by identifier at `exact` (`uses` takes no expression), with the reason `@<ref>
(indexed <short commit>)` and `, ref mismatch` only for a ref of 40 or 64 hexadecimal digits
other than the provider's indexed commit. The Git ref is what follows the `@` of the consumer's
`via`: never part of the identifier, never resolved. The indexed commit is
`index::Export::commit`, which `Engine::commits` sets for the repositories that hold a provider
of a kind whose `reads_commit` says so: the checkout's `HEAD` locally, the commit recorded
beside the index in server mode, the generation's `head` at a generation.

`context_at` renders `provides <contract>` with one `  consumer:` line per link under a
pipeline, and `consumes <contract> @<ref>` with its `  provider:` lines under a CI job. The keys
a call passes (`with`, `secrets`, `inherit` as that word) follow, sorted. `written` copies the
Git ref and those keys to the consumer rows of `meada export` (`ref`, `with`, `secrets`, each
left out when empty). `search` with the kind `ci` is this kind's own renderer (`own_search` over
`iac::actions::hits`): it lists CI nodes, while the kind `contract` lists the contracts, each
side placed as `context` takes it (`place`). `in_default_search` keeps them out of a search with
no kind, which answers as before. The kind is not `decidable`: it has no ambiguous link.
`calls_from_definition` says a CI job calls its pipeline, so `trace` never walks a CI link as
data going from a writer down to its readers.

`trace` over CI calls (#825) is the IaC reference walk (`iac::trace`), which now takes the
links a node holds in the walk's direction from `trace::Graph::node_calls`: a link whose
consumer calls its provider, both ends IaC nodes, the consumers going up and the provider going
down. It follows them before the node's references and goes on through the reached node's own
references, in that node's repository, so a chain crosses intermediate reusable pipelines. The
walk's cycle and repeat marks key a node by repository, root and address. A CI contract
identifier starts at the node that provides it (`trace::provider_node`, read in
`Engine::trace` before the start is looked up), so it answers as the pipeline's name does; the
walk adds no cap for GitHub's ten levels of nesting.
