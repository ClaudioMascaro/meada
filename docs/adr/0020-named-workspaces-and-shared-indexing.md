# 20. Named workspaces and shared indexing: the direction of spec S5

- Status: accepted (direction only; spec S5 settles the contracts)
- Date: 2026-10-23

## Context

ADR 0014 keeps one workspace per deployment and rejects "Several workspaces per fleet" (a
switcher, roles per workspace, one key prefix per workspace). ADR 0015 rejects the same option
for teams: one workspace is one link universe, and a team's need is to change its own entries,
not to be isolated from the others.

Triage of spec S5 (#339) on 2026-10-23 accepted a direction that does not contradict the reason
those rejections give, because it separates two things the rejected option fused: what a query
reads, and what is indexed and published. This ADR records the direction before slice #322
widens the workspace version entry with `owner`, so that the entry's schema does not shut the
direction out. It fixes no runtime, key, route, role rule or file format: spec S5 owns those.

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
  that several names hold. Until S5 merges, ADR 0014's one workspace per deployment stands.
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
