# Named workspaces: acceptance (#347)

`tests/it/ui/oidc/workspaces/acceptance.rs` runs on the same case as `workspaces.rs` and forces the
stages of a refresh to fail in turn, with a node serving the first generation: the role check
(GitHub answering 500) exits non-zero and writes no set, so the pointer and the catalog stay; a
revocation committed by a later complete check is effective at the node while the generation is
unchanged; a role check that fails again while GitHub already shows the role restored brings
nothing back; a missing member mirror stops `index` (an entry left without a sponsor needs none)
and a missing index directory stops `snapshot publish`, each leaving the pointer on the first
generation and the other group's workspace answering; only a later complete set restores the
grant. `Case::grant_all_but` and `Case::await_check` (in `lifecycle.rs`) are the revocation helper
split in two. `scripts/helm-check` renders `values-workspaces.yaml` and checks the sections are in
the console's and the job's bootstrap and not a node's, and the three refusals of the chart. The
site's named-workspaces page maps each acceptance criterion to the tests that show it.
