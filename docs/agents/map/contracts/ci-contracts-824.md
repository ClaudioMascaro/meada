# CI contracts (#824)

`ci.rs` owns `ci::<repository identity>/<path>` (ADR 0028): a reusable pipeline as provider, each
CI job that calls it as consumer. It extracts nothing at index time: both sides are read at
query time, in `workspace_step`, from the CI nodes `iac/actions.rs` stores, because a call is a
consumer only when the repository it names is one of the workspace, which the index of one
repository cannot know. The provider sits on the file node `.` whose `iac::actions::Facts` says
`reusable` (`on` holds `workflow_call` as a string, a list item or a map key). The consumer sits
on the `job` node whose `uses` reads `<owner>/<repo>/.github/workflows/<file>@<ref>`
(`Uses::read`, `consumed`). A `uses` writes no host, so the host is the one of the consumer's identity, with
owner and repository lower-cased as an identity is and the path as written; a repository
without `origin` (`local/...`) names no peer. A call whose repository is not in the workspace
holds no side at all: nothing lists it as an orphan or an open item. One whose repository is
there and whose path names no provider is a consumer with no link, marked absent (#827, below).

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

The other forms of `uses` (#826) go through the same reader: `Uses` is `Peer` (owner, name,
path, Git ref), `Rooted` (`$/<path>`) or `Local` (`./<path>`), and `consumed` gives the contract
of a call for a CI job (a pipeline file) or a CI step (the folder of an action, nothing for the
root), with the path normalised by `iac::joined`. An action's file node provides
`ci::<identity>[/<folder>]`. A call inside its repository links with the reason `own
repository` and no ref. A CI step's `./<folder>` is unresolved wherever `workspace_causes` names
a cause (a composite action, a reusable pipeline, no `actions/checkout` before it in its CI
job, one that names `repository` or `path`): its consumer then has no contract. The kind hook
`unresolved_causes` hands those sites to `items::list` (no candidate, `OpenItem::cause`) and to
`Engine::search` (no line in the default search or under the kind `contract`); `search` with
the kind `unresolved`, `context` on the CI step and `meada decide --list` show the cause.

Orphans and the absent mark (#827): `scope_status` gives `meada status` one `Msg::OrphanCi` line
per provider of the repository that no consumer of the visible exports consumes, and nothing
while those exports hold no CI consumer with a contract (the scope rule of ADR 0026). `absent`
names the mark of a consumer whose contract no export provides, with its second reason when the
path is a pipeline's file node that is not reusable. `side_note` hands it to the engine for
`search` kind `contract` and `context` on a contract identifier, `context_at` appends it to the
`consumes` line, and `written` sets `absent` on the consumer rows of `meada export` that hold no
link. `index::Export::dockerfiles` (table `dockerfiles`, rewritten when the walk's set of
folders with a file named `Dockerfile` differs) lets `workspace_step` drop the consumer of a
call to such a folder with no action file: GitHub builds that action and Meada reads none.
