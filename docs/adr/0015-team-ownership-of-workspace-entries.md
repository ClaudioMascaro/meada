# 15. Team ownership of workspace entries

- Status: proposed
- Date: 2026-10-22

## Context

Server mode serves one workspace per deployment (ADR 0011), and ADR 0014 keeps it: one bootstrap
manifest, one key prefix, one current workspace version, moved by `meada workspace apply` or by
the console's editor. In the console, a subject in a `[ui] operator_groups` group, or whose
`operator_claim` is `true`, is an operator and may change every entry and every generation;
every other admitted subject is a viewer.

What a token reads is already bounded per team. Its grant is the union of the identities
`identities_claim` lists and those its groups map to in `[oidc.groups]` (ADR 0012, #290, #291),
a repository outside the grant is absent from every answer, and a cross-repository link shows to
a token granted on both ends. What a team may change is not: nothing records which team owns a
repository, and a team that adds or edits its own entry needs an operator of the whole
workspace, who then holds every change of every team.

Issue #294 first asked for several workspaces per fleet, one per team. Triage on 2026-10-22 set
that aside with the maintainer: the teams of one organisation share links across their
repositories, which one workspace gives and several would split, and their need is to change
their own entries, not to be isolated from each other. Writing spec S3 (#294) settled the
remaining points. This ADR records the decisions; the spec fixes the details the slices build.

## Decision

### One workspace, several owners, shared trust

- Several teams share one workspace and one fleet under one operator. Trust is shared: isolation
  between teams as a security boundary is not a goal, and the global operator answers for the
  whole deployment.
- One workspace is one link universe. Links cross teams as they do today, filtered by the grant.
  A repository appears once in the workspace and has one owner.
- Routes (`/<identity>`, `/<identity>@<generation>`), the key prefix, the store prefix,
  `--workspace`, the refresh lock, the fence, the job and the heartbeats stay as ADR 0014 leaves
  them.

### Teams and owners

- A team is an OIDC group: a name from the token's `groups_claim`, the claim `[oidc.groups]` and
  `operator_groups` already read. Meada keeps no team table; teams and their members are managed
  in the identity provider.
- Each `[[repository]]` entry of a workspace version may carry `owner = "<group>"`, one group.
  The name is a string of 1 to 256 characters with no control character and no whitespace at
  either end, compared exactly, case included, as `[oidc.groups]` keys and `operator_groups` are
  compared. `meada-core` checks it with the other entry checks, so the browser's editor and every
  save, import and pull check it alike.
- `owner` is part of the entries' canonical text, so it changes the workspace version id. An
  entry without it has the canonical text it had before this ADR, so a stored version keeps its
  id. It is not part of generation or bundle ids: indexing never reads it.
- Changing an owner is an apply, recorded with its author and message like any other.
- `owner` lives only in workspace versions. In a bootstrap manifest's own `[[repository]]`
  entries (`source = "file"`) it is an error naming the key and `source = "store"`, since a
  static manifest has no editor and no one to delegate to. `meada workspace import` and the
  console's save accept it. A repository's `meada.toml` replaces an entry's blocks, never its
  owner, and has no `owner` key.

### Roles

- The console has three roles. The **global operator** is ADR 0014's operator: a subject in an
  `operator_groups` group, or whose `operator_claim` is `true`. A **team operator** is any other
  admitted subject one of whose groups owns an entry of the current workspace version that the
  subject's grant reads; those groups are its teams. Every other admitted subject is a viewer.
- Team operators exist only with `[oidc]` carrying `groups_claim`, `[ui] operations = true` and
  `[workspace] source = "store"`. Without them, owners are recorded and shown, and nobody is a
  team operator.
- A group becomes a team when the global operator assigns it its first entry: that is the
  delegation. A team that removes its last entry is no longer one, until the global operator
  assigns it an entry again.
- The role is computed per request from the token's groups and grant and the current version,
  as the grant is computed at admission.

### What a team operator may change

- A team operator may edit or remove an entry whose owner is one of its teams, and add entries
  whose owner is one of its teams. The global operator keeps everything else: changing an owner
  (setting, clearing or replacing it), adding, editing or removing an entry with no owner,
  touching another owner's entry, and every generation operation (`meada snapshot pointer set`,
  `retain` and `drop`, `meada snapshot fence recover`).
- Ownership never widens the read grant. A team operator sees the workspace as a viewer with its
  grant does, and an entry outside its grant is neither shown to it nor changeable by it, even
  one its team owns. The operator maps a team's group in `[oidc.groups]` to the identities the
  team owns, and to an identity before the team can add it: an addition outside the grant is
  refused alike whether or not the identity is already in the workspace.
- Under the GitHub grant provider of #319, where a subject's grant also holds what its GitHub
  login reads on GitHub, an addition is not refused as outside the grant: a repository outside
  the workspace has no permissions listed yet. The entry records the adder's GitHub login, and
  the job fetches it only while that login holds one of the accepted roles on the repository,
  checked at every refresh. Who reads it is decided by GitHub's roles, as for every entry.
  (Amended 2026-10-22; #319: ADR 0016 spells the rule out. The entry's `added_by` is the adder's
  login, which the console sets from the saver's login claim and never from its text; an
  addition on the provider's host without a login is refused `unattributed`, a team operator's
  change of `added_by` is refused `added_by_change`, and `outside_grant` is not applied to an
  addition on the host that carries `added_by`, with one exception: an identity the workspace
  already holds outside the grant is refused `outside_grant`, named as the subject submitted it,
  which tells the team operator it is present, accepted under shared trust. The job's role check
  writes the exclusions before the fetch, and the global operator may set, clear or replace
  `added_by`.)
- A change is authorized by its diff against the current version, entry by entry, keyed by
  identity. A refused change is a 403 that names every entry it may not change, with a reason
  each, and writes nothing. It never names an identity outside the subject's grant: those are
  counted, not named.
- The authorization is one function of `meada-core`, taking the subject's role, groups and grant,
  the current version's entries and the new entries, and returning the refused entries with
  their reasons. The console's write routes call it, and the browser's editor calls the same
  function compiled to WebAssembly to mark refusals before the save.
- A team operator's save is the entries it reads: the new version keeps every entry outside its
  grant as it is, in place, so a save never drops what the team operator cannot see. Its apply of
  a stored version is authorized against the whole current version, under the refresh lock.
- The CLI twins (`meada workspace import` and `apply`) act as the global operator: they hold the
  store's and Valkey's credentials, which already allow every write.
- Every write's stderr line names the subject and, for a team operator, the owner groups that
  authorized it.

### What stays out of this ADR

- What one repository's failed fetch or index does to the refresh, which #316 decided in ADR
  0012: the whole publication stops and the current generation stays, so a repository one team
  owns stops new generations for every team until its cause is repaired, and no repository is
  left out or carried over from an earlier generation.

## Considered options

- **Several workspaces per fleet, with routes per workspace.** One key prefix, one current
  version and one console switcher per workspace, and a route that names the workspace. It splits
  the link universe the teams share and multiplies the operator's state, for a separation that
  shared trust does not need. ADR 0014's one workspace per deployment stands.
  (Amended 2026-10-23; #322: ADR 0020 records the direction of spec S5 (#339), a named query
  context over shared indexing and publication, which neither splits the link universe's
  indexes nor multiplies the operator's publication. The rejection stands for isolated
  workspaces; spec S5 owns the runtime and the final contracts, and the ownership of entries
  here is unchanged.)
- **A federation of workspaces**, where each repository has one owning workspace and the others
  read it from that workspace's generation. Cross-workspace links would need generations of
  several workspaces at once on every node, a pin that names several generations and a fence per
  workspace. Ownership of entries in one workspace gives the same delegation without it.
- **Multi-tenant isolation.** What one tenant may learn about another (the fleet's state, the
  versions' authors, the existence of a repository) would become a security boundary across the
  console, the nodes and the job, which runs every fetch with one set of credentials. Not a goal
  under shared trust.
- **A team table in Meada.** Members, names and lifecycles kept beside the identity provider's
  groups, with a second place to revoke someone. The OIDC group is the team.
- **Team roles as a `[ui]` key** (a list of groups allowed to act as teams). A second list to keep
  beside the owners: an owner already names the group, and assigning its first entry is the
  delegation.
- **Ownership widening the read grant.** Owning an entry would show its configuration to a token
  the grant does not admit, and the console would have two rules for what a subject reads. The
  grant stays the one rule; the operator maps the team's group in `[oidc.groups]`. #319 widens
  the grant itself, from GitHub roles, never from ownership.
- **Ownership in the bootstrap manifest.** A static manifest changes by pull request and redeploy,
  reviewed in the ops repository, and has no editor through which a team could act on it; an
  owner there would mean nothing. It is an error, so the key never looks effective where it is
  not.
- **Ownership read from the git host** (CODEOWNERS, GitHub teams). The console would call GitHub,
  against ADR 0012's outbound rule and ADR 0014's console that never calls it, and ownership would
  depend on one forge and on files inside the mirrors. The owner is a fact of the workspace
  version, with an author and a message. #319 reads GitHub roles for the grant, not for
  ownership, and in the job, which already calls GitHub (ADR 0013), not in the console.
- **Several owners per entry.** Two teams editing one entry would need a rule between them; one
  owner keeps the diff's answer simple, and a shared repository stays the global operator's.
- **Teams moving generations.** The pointer, the retained set and the fence are the fleet's, not a
  repository's: a team's rollback would move every team. They stay with the global operator.

## Consequences

- What each invariant of ADR 0002 keeps, with no new amendment of it:
  1. Indexing never touches the network: `meada index` reads the pulled version from disk as
     ADR 0014 says and ignores `owner`.
  2. Source stays within the operator's boundary: a team operator reads only what its grant
     admits, and the console connects to nothing new. An entry a team operator adds is fetched
     by the job with the job's credentials (a GitHub App installation or the job's git
     credentials), and subjects with a `*` grant read it. This is accepted under shared trust and
     written in the documentation.
  3. Meada writes only in its own state: an owner lives in the workspace version documents and
     the Valkey workspace pointer ADR 0014 already writes. No new key, prefix or file.
  4. The MCP server has no tool that writes: the nodes are unchanged. The console's writes stay
     an operator's explicit commands, now also a team operator's on its own entries.
  5. One index per repository: unchanged; a repository appears once and has one owner.
  6. Determinism: the version id derives from the entries, owners included; generation and
     bundle ids do not change with an owner, so an apply that only changes owners makes no new
     generation. The authorization is a pure function of its inputs, and the browser and the
     server give the same answer.
- ADR 0014 carries inline amendments: its roles gain the team operator, and its rejected option
  "Several workspaces per fleet" points here.
- ADR 0012's rule that the grant's mapping lives with the operator, not in a `[[repository]]`
  entry, stands: an owner grants no read. It holds for the claims grant; #319 adds GitHub roles
  as a second source, unioned with it.
- The operator owns more: mapping each team's group in `[oidc.groups]` to the identities it
  owns, assigning a team its first entry, and accepting that a team may add a repository the job
  fetches with the fleet's credentials. One team's failing repository stops new generations for
  every team until it is repaired, while the nodes keep the last complete generation (#316, ADR
  0012).
- Everything is additive under Spec R1's stability rule (a key of the workspace version entry, a
  role, a refusal of the API): a minor change. Without owners, the console behaves as ADR 0014
  says.
- S3's slices widen S2's: the entry schema and canonical text (#301), apply and its refusal
  (#302), admission computing the team operator role (#306), the write routes and their stderr
  line (#307), the editor and its diff (#310), and the documentation and console profiles
  (#314). They are cut once #302, #306 and #307 merge.
