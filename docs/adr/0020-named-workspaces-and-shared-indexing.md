# 20. Named workspaces and shared indexing: the direction of spec S5

- Status: accepted
- Date: 2026-10-23
- Amended 2026-10-30 (#340): the contracts spec S5 owed are settled. This ADR now also holds the
  domain decision (roles, eligibility, the Y/A/B rule, discovery, revocation); ADR 0021 holds the
  persistence, publication, query and configuration contracts, ADR 0022 the proposal interface.
  The direction below is unchanged; "direction only" no longer describes it.

## Context

ADR 0014 keeps one workspace per deployment and rejects "Several workspaces per fleet" (a
switcher, roles per workspace, one key prefix per workspace). ADR 0015 rejects the same option
for teams: one workspace is one link universe, and a team's need is to change its own entries,
not to be isolated from the others.

Triage of spec S5 (#339) on 2026-10-23 accepted a direction that does not contradict the reason
those rejections give, because it separates two things the rejected option fused: what a query
reads, and what is indexed and published. This ADR records the direction before slice #322
widens the workspace version entry with `owner`, so that the entry's schema does not shut the
direction out. It fixes no runtime, key, route, role rule or file format: spec S5 owns those. (Amended 2026-10-30;
#340: ADR 0021 and ADR 0022 fix them and this ADR holds the role and eligibility rules.)

## Decision

- A **named workspace** is a named query context: a name under which a query reads a chosen set
  of the repositories a deployment serves, and the links between them. It decides what an answer
  can see.
- **Shared indexing** and publication are not per name. Indexing a repository and publishing its
  bundle and generation stay what ADR 0012 and ADR 0014 make them, so one name never costs a
  second index of the same repository at the same commit, and `meada index` still opens no
  connection and reads no name.
- The two are kept apart in code, documents and the glossary: naming a context says nothing
  about how a repository is indexed or published, and sharing an index says nothing about who
  may read it. The read grant stays the one rule for what a token reads (ADR 0012, ADR 0015).
- Spec S5 (#339) owns the runtime and the final contracts: how names are declared, routed and
  stored, which roles act on which name, and the rules for the first workspace and for entries
  that several names hold. Until S5 merges, ADR 0014's one workspace per deployment stands. (Amended 2026-10-30; #340: the
  contracts are decided and the stand continues until the runtime ships, for every deployment
  without `[workspaces]`.)
- Nothing is renamed here: existing role values and commands keep their names.
- An entry's `owner` (ADR 0015) is entry metadata of a workspace version. It is not an access
  control list of a workspace and not a source grant, and no helper that reads or checks it
  assumes one user-visible workspace.

## Considered options

- **Several full workspaces per fleet** (the option ADR 0014 and ADR 0015 reject): each with its
  own key prefix, current version, indexes and publication. It still costs what they say, and
  indexes the same repository once per workspace. Not taken.
- **Leave names out of S5**: keeps ADR 0014 as it is and makes a team that wants a narrower view
  of the fleet run a second deployment. Not taken; S5 may still decide a name is not worth its
  cost.

## Consequences

- ADR 0014 and ADR 0015 carry dated amendments on their rejection of several workspaces per
  fleet: the rejection stands for isolated workspaces with their own indexing and publication,
  and no longer for a named query context over shared indexing.
- `GLOSSARY.md` defines **Named workspace** and **Shared indexing**; **Workspace** keeps its
  meaning of what one query sees.
- Slice #322 preserves the ownerless canonical text, keeps `owner` out of the indexing blocks
  and out of bundle and generation ids, and puts no check of it behind a single workspace.

## Final decision (2026-10-30; #340)

Spec S5 (#339) was consolidated against the merged S3 (ADR 0015) and S4 (ADR 0016) code. Nothing
below reopens the agreed repository-authority rule or the Y/A/B rule; where the direction left a
point open, this section closes it. Nothing here is implemented yet: current behavior is still
ADR 0014's one workspace per deployment, and the glossary says which terms are delivered.

### The three human roles

- **Platform administrator** is ADR 0014's operator, the **global operator** of ADR 0015: a
  subject in an `operator_groups` group, or whose `operator_claim` is `true`. It is a new name
  for an existing authority, not a new role: no serialized value is renamed or added.
- **Workspace operator** is authority over one named workspace, held by the subject that created
  it and by the subjects the platform administrator delegates it to. It is a fact about a pair
  (subject, workspace), recorded in the workspace's operator list, never a session role and never
  a value of `role` in the session, the audit record, the refusal JSON or WebAssembly. A subject
  can be a viewer in its session and the operator of three workspaces. It is not the **team
  operator** of ADR 0015, which acts on owned entries of the deployment workspace and nowhere
  else.
- **Inherited viewer** is not a role anyone is given. A subject views a named workspace when its
  own current grant reads the workspace's entries (the discovery rule below). There is no viewer
  list, invitation, group enrollment or access package: the repository grants are the only
  input, and group peers are viewers because they hold the same roles, not because of the
  creator.
- Serialized-role compatibility: `operator`, `team_operator` and `viewer` keep their meaning in
  the session, `Role` in `meada-core`, `authorize_entries`, the audit's `role` column and the
  front end. A named-workspace write records the authority it used in the audit record's
  `arguments` (`workspace`, `authority`: `platform_administrator` or `workspace_operator`),
  beside the unchanged `role`. S3's `owner` stays what ADR 0015 and ADR 0020 above say: entry
  metadata of the deployment workspace, an owner-clear is still `owner_change`, and a legacy
  ownerless entry stays ownerless for that check. A named workspace's entries carry no `owner` and
  no `added_by` (ADR 0021).

### Creation eligibility and what a viewer reads

- **Eligible for creation** means: the subject's GitHub login holds, in the newest fresh
  permissions set, one of the accepted roles of `[oidc.github]` on a repository the catalog
  covers (ADR 0021). A role held directly and a role the repository gives a group the login
  belongs to are the same row, since GitHub lists effective collaborators (ADR 0016), so no
  feature outside a provenance display needs to tell them apart. A GitHub team is never matched
  to an identity-provider group by name.
- The claims grant (`identities_claim`, `[oidc.groups]`) does not make a repository eligible:
  creation derives from accepted repository roles, the rule the maintainer approved. The claims
  grant still counts for **reading**, since the grant is the one rule for what a token reads
  (ADR 0012, ADR 0016). So a subject whose claims grant covers Y and A and holds no role can
  view a workspace of Y+A and cannot create one.
- A **workspace operator** adding or changing an entry needs that entry eligible for itself at
  the moment of the write. The **platform administrator** is not bound by eligibility when it
  edits a workspace, as the global operator is not bound by owners (ADR 0015), and is bound by
  the catalog. What the administrator adds is fetched only while an operator of the workspace
  holds an accepted role on it (the sponsor rule, ADR 0021), so the fleet's credentials never
  fetch a repository for a workspace whose operators GitHub does not let read it.
- No eligibility is stored. Each decision reads the newest set; a role lost is lost at the next
  set, and an old set never revives: a newer set always decides, and a set past `max_age_seconds`
  decides nothing (fail closed, as ADR 0016's grant).

### The Y/A/B matrix, frozen

Y grants an accepted role to groups A and B, repository A to group A only, repository B to group
B only. Callers hold effective rows: `alice` (group A: Y, A), `bob` (group B: Y, B), `carol`
(both groups: Y, A, B), `dave` (the same rows as `alice`, granted directly), `erin` (a group A
peer of `alice`), `yoda` (Y only) and `zed` (only a role the policy does not accept).

| Caller | Creates Y+A | Creates Y+B | Creates Y+A+B | Creates Y |
| --- | --- | --- | --- | --- |
| alice, dave | yes | refused, 1 hidden | refused, 1 hidden | yes |
| bob | refused, 1 hidden | yes | refused, 1 hidden | yes |
| carol | yes | yes | yes | yes |
| yoda | refused, 1 hidden | refused, 1 hidden | refused, 2 hidden | yes |
| zed | refused | refused | refused | refused |

- A refusal names the ineligible identities the caller's own grant reads and counts the rest
  (ADR 0015's `outside_grant` rule), so a caller never learns of a repository it cannot read.
  In the table every refused identity is outside the caller's grant and is counted; `zed` holds
  only a role the policy does not accept, so all of its selection is counted.
- Granting Y never grants A or B. A cross-repository link from Y to B in the workspace of Y+A is
  absent for every caller: B is not an entry of that workspace, and a link needs both endpoints
  in the selected set and in the caller's grant.
- Two workspaces over Y are two contexts of one index. Y is indexed and published once.
- The table is `tests/fixtures/named-workspaces.json`, read by a `meada-core` test today and by
  the runtime's Rust, public-process and WebAssembly tests when they land. The runtime tickets
  extend the table; they do not rewrite a row without an ADR amendment.

### Creator versus peer

- The creator is the first operator. A group peer has no authority over the creator's workspace:
  it can view it, and it can create a different workspace of its own if it is eligible. Only the
  platform administrator delegates another operator. Operators are OIDC subjects (as the
  console records authors) and their login as of their last accepted write,
  which the sponsor rule reads.
- Mutation needs both facts: authority (operator of that workspace, or the platform
  administrator) and, for an operator, the eligibility of every entry it adds or changes. Source
  permission alone never edits another creator's manifest.

### Discovery of a workspace by a partial reader: hidden

A workspace is **hidden** from a subject that does not read every entry of its current version.
The decision, and its reasons:

- A non-operator subject sees a workspace (its existence, name, description, entries, versions,
  diagnostics and graph answers) only when its grant reads **every** entry of the workspace's
  current version, excluded entries included (ADR 0021). Otherwise the workspace does not exist
  for it: a list omits it and a show or a query gets the same 404 an unknown id gets. (Amended
  2026-10-30; #510. The first version said "excluded entries aside", which let a subject that
  reads Y alone see a Y+A workspace once A lost its sponsor, against the reasons below and the
  revocation table. An excluded entry is not fetched, but the workspace still names it.)
- Filtering was rejected because a name or description is not bounded by repository grants: a
  title such as "payments-and-fraud-core" discloses repositories the caller cannot read, and a
  filtered entry list still discloses their count. Hiding the whole is the only rule that
  reveals no inaccessible entry, name or description.
- Workspaces are therefore identified by a deterministic id, not by a unique name (ADR 0021):
  names repeat freely, so creating one reveals nothing about hidden workspaces.
- Operators and the platform administrator are exempt, so that a creator whose role on A was
  revoked still reaches its own workspace, sees the entries it may read plus a **count** of the
  hidden ones, and can repair or delete it. An operator exempt in discovery is still filtered by
  grant in every graph answer.
- A subject that holds only Y is not left unable to read Y: it is eligible to create a workspace
  of Y alone, which the other subjects with Y can view.

### Revocation after creation

| Event | Creator (operator) | Group peer (viewer) | Ingestion |
| --- | --- | --- | --- |
| Role on A lost, or removed from the group | keeps the workspace; A is a hidden count; may edit and remove the entries it reads, roll back to a version it reads, or delete; may not add or change entries it is not eligible for; the platform administrator removes A | the workspace becomes hidden | A is excluded `no_sponsor` unless another operator holds a role on it |
| Role on Y lost | as above, for Y | as above | as above |
| Set older than `max_age_seconds` | read-only: nothing can be created or extended | the grant is empty, so workspaces are hidden | unchanged until the next complete refresh |
| Role restored | nothing to restore: the next set decides | visible again with the next set | A is eligible again at the next refresh |
| Subject deleted by SCIM | blocked in the console as before; its operator rows remain | admitted as before; its roles lapse with GitHub's | sponsor logins lose their role at GitHub's pace |

The workspace, its versions and its history are never deleted by a revocation. The console's
durable block of a SCIM-deleted user (ADR 0016's revocation marker) holds for named-workspace
routes, and only an explicit active reprovisioning lifts it. A never-provisioned subject keeps
normal admission.

### Preserved decisions

A fetch or index failure aborts the whole shared publication and the current generation stays
(ADR 0012, #316): every named workspace keeps the last complete publication, and one repository's
failure stops new generations for every workspace, as ADR 0015 accepts for teams. An eligibility
exclusion is a decision before the fetch, not a failure. A permissions set committed before a
graph failure keeps its own freshness and revocation.

## Amended consequences (2026-10-30; #340)

- ADR 0021 settles the five contracts spec S5 lists as distinct (catalog, selected entries,
  shared ingestion membership, version lifecycle, query selection), the effective configuration
  and the deterministic ids. ADR 0022 records the authenticated proposal interface.
- The one-workspace decisions of ADR 0014 and ADR 0015 stand for a deployment without the
  `[workspaces]` section of ADR 0021. With it, the **deployment workspace** (ADR 0014's one
  workspace) keeps every behavior it has, and named workspaces are added beside it.
- ADR 0002 keeps its letter. Invariant 4 is read in ADR 0022: the proposal server's tools write
  nothing, and the node query server gains no tool.
