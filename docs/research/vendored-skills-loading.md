# Vendored skills: how AI Hero intends mattpocock/skills to be used, and how Meada should carry them

- Date: 2026-11-01
- Question: How does AI Hero (Matt Pocock, aihero.dev) intend `mattpocock/skills` to be installed,
  loaded and updated, how does Claude Code load what Meada vendors today, and is there a better
  way for this repository to carry the skills, given that they stay ("fazem parte do framework")
  and that the backlog loop's cloud sessions re-read the prompt about 120 times per ticket?
- Outcome: Meada's setup is the upstream "editable files" route with a better lock than the
  upstream tool's; the managed plugin route does not fit (it drops the four local edits, renames
  every skill to `mattpocock-skills:<name>` and does not reach cloud sessions without a setup-script
  install). The vendored set costs far less than assumed: 3,252 bytes (15 entries) of the listing
  in a loop session and 2,304 bytes (11 entries) in a local one, because the 23 user-invoked skills
  are not listed at all. A cloud probe confirmed it: of the 20.5 KB of listing text in an
  Anthropic-hosted session, 15.6 KB are Anthropic's and Claude Code's bundled skills (section 4).
  Recommendation: keep vendoring; drop the four frozen `misc/` skills (or move their
  `skillOverrides: off` into the committed `.claude/settings.json`), which takes the loop's share to
  2,304 bytes; the larger lever is the bundled skills (section 6).

## 1. Method

Primary sources only: the `mattpocock/skills` repository at `main` (`4ae2317`, 2026-10-31), read
through the GitHub API; aihero.dev pages; the `skills` CLI repository (`vercel-labs/skills`); Claude
Code's own documentation at code.claude.com. Listing sizes come from the `skill_listing`
attachments in local session transcripts (`~/.claude/projects/<project>/*.jsonl`, field
`attachment.content`), split per entry, and from the frontmatter of each vendored `SKILL.md` in a
checkout of `main` (`84e9c87`). Bytes are UTF-8 bytes of the listing line `- <name>: <description>`.

## 2. How AI Hero intends the skills to be used

### 2.1 Install and update routes

- The repository's canonical install text lives in `.agents/install-block.md`, and its own rule is
  that `README.md`, changesets and every page under `docs/` "must say **this** and nothing else"
  (https://github.com/mattpocock/skills/blob/main/.agents/install-block.md). Its policy is
  "managed installs first": routes that update themselves lead, and skills.sh is the fallback,
  "labelled 'manual updates'".
- Claude Code: `claude plugin install mattpocock-skills@claude-plugins-official`, "Updates itself
  by default" (README, https://github.com/mattpocock/skills). The README warns: "Pick one per agent,
  because installing both gives you every skill twice."
- Editable files: `npx skills@latest add mattpocock/skills -a <agent>`, updated with
  `npx skills@latest update`, re-running `add` for new skills (README). The README asks to include
  `setup-matt-pocock-skills` when the installer offers the list.
- After either route: run `/setup-matt-pocock-skills` once per repository (README). Meada's
  `docs/agents/issue-tracker.md`, `triage-labels.md` and `domain.md` and the `## Agent skills`
  block in `AGENTS.md` are that skill's output (`.agents/skills/setup-matt-pocock-skills/SKILL.md`,
  sections 3 and 4).
- aihero.dev/skills presents it the other way round: the npx installer as "Recommended" ("Pick the
  skills and agents you use. The installer writes editable files into your project.") and the
  plugin as "a managed, read-only plugin from the official marketplace" that "Updates
  automatically" (https://www.aihero.dev/skills). That page also names older skills (`/to-prd`,
  `/to-issues`, `/domain-model`) that the repository has since renamed (`to-spec`, `to-tickets`,
  `domain-modeling`), so the repository's install block is the current source.

### 2.2 What each route ships

- The plugin is curated: `.claude-plugin/plugin.json` (version 1.3.2) lists 27 skill directories
  from `engineering/` and `productivity/` only
  (https://github.com/mattpocock/skills/blob/main/.claude-plugin/plugin.json). ADR 0002 says the
  `misc/`, `personal/`, `in-progress/` and `deprecated/` buckets "are **not**" promoted
  (https://github.com/mattpocock/skills/blob/main/.agents/adr/0002-ship-as-a-claude-code-plugin.md).
- The official marketplace entry pins a commit: "the **sha is pinned**, so a release reaches
  installed users when that pin moves, not the moment we tag" (ADR 0002, update 2026-08-05).
- `in-progress/` is "Beta", "excluded from the plugin", installed one by one with
  `npx skills@latest add mattpocock/skills --skill=<name>`
  (https://github.com/mattpocock/skills/blob/main/skills/in-progress/README.md).
- `misc/` is "frozen and unmaintained. Every issue is closed"
  (https://github.com/mattpocock/skills/blob/main/SCOPE.md), "Tools I keep around but rarely use,
  not promoted in the plugin" (https://github.com/mattpocock/skills/blob/main/skills/misc/README.md).
- Name collisions with harness built-ins (`/research`, `/code-review`, `/prototype`) are out of
  scope upstream; the answer is the plugin's namespace (`/mattpocock-skills:code-review`) or, for
  skills.sh copies, "the files are yours: rename the folder and the `name:` field"
  (https://github.com/mattpocock/skills/blob/main/.out-of-scope/harness-name-collisions.md).

### 2.3 The invocation split

Upstream splits every skill on one axis (https://github.com/mattpocock/skills/blob/main/.agents/invocation.md):

- **User-invoked**: "reachable **only by the human typing its name**", marked with
  `disable-model-invocation: true`. These orchestrate.
- **Model-invoked**: the default, reachable by model or user; "The `description` is
  **model-facing**".
- Skills depend on each other only through "an explicit instruction to **call the Skill tool**
  with the named skill", and only when the target is model-invoked: "A user-invoked skill can never
  be reached this way".

The vendored `writing-for-agents/SKILL-MECHANICS.md` gives the cost model: a user-invoked skill has
"Zero context load, but it spends cognitive load"; a **router skill** is "one user-invoked skill
that names the others", a cure for cognitive load, and "It can only hint, never fire them".
`ask-matt` is upstream's router (`disable-model-invocation: true`).

### 2.4 The `skills` CLI

- Claude Code's project path is `.claude/skills/`, global `~/.claude/skills/`; most other agents
  use `.agents/skills/` (https://github.com/vercel-labs/skills/blob/main/docs/agents.md).
- Install method: "Symlink (Recommended)", "symlinks from each agent to a canonical copy", or
  `--copy`; `-s, --skill` selects skills by name; `-g` installs to the user directory
  (https://github.com/vercel-labs/skills/blob/main/docs/commands.md).
- `skills check` "exits with code 1 when any skill has an update, so it can gate CI"; `skills update
  [names]` refreshes (same page).
- Its lock is `skills-lock.json`, "meant to be checked into version control", recording per skill
  `source`, `ref` (branch or tag), `sourceType`, the `SKILL.md` path and a `computedHash`, a
  "SHA-256 hash computed from all files in the skill folder"
  (https://github.com/vercel-labs/skills/blob/main/src/local-lock.ts). It records a ref, not a
  commit, and has no field for "this copy was edited on purpose".

## 3. How Claude Code loads skills

From https://code.claude.com/docs/en/skills unless noted.

- Sources: personal `~/.claude/skills/<name>/SKILL.md` ("All your projects on this machine, but
  not Cowork or cloud sessions"), project `.claude/skills/<name>/SKILL.md`, nested
  `<subdir>/.claude/skills/`, plugin skills "as `/plugin-name:skill-name`". "Enterprise over
  personal, and personal over project." `.agents/skills` is not a Claude Code source; Meada's
  `.claude/skills/<name>` symlinks are what make them project skills.
- What enters the prompt: "skill descriptions are loaded into context so Claude knows what's
  available, but full skill content only loads when invoked". The per-entry text is `description`
  plus `when_to_use`, "truncated at 1,536 characters in the skill listing".
- The invocation table:

  | Frontmatter | You can invoke | Claude can invoke | When loaded into context |
  | :- | :- | :- | :- |
  | (default) | Yes | Yes | Description always in context, full skill loads when invoked |
  | `disable-model-invocation: true` | Yes | Not on its own | Description not in context, full skill loads when invoked |
  | `user-invocable: false` | No | Yes | Description always in context, full skill loads when invoked |

  `disable-model-invocation`: "Set to `true` to prevent Claude from automatically loading this
  skill. [...] Also prevents the skill from being preloaded into subagents". `user-invocable:
  false` only hides the skill from the `/` menu; it saves nothing. The transcripts confirm the first
  row: no user-invoked Meada skill appears in any measured listing, name included (section 4).
- `skillOverrides` (scope "Any file", https://code.claude.com/docs/en/settings-reference): `"on"`
  lists name and description, `"name-only"` the name only, `"user-invocable-only"` hides it from
  Claude but keeps it in the `/` menu, `"off"` hides it from both. "Plugin skills are not affected
  by `skillOverrides`."
- Budget: the listing "scales at 1% of the model's context window"; when it overflows, Claude Code
  "drops descriptions starting with the skills you invoke least". Knobs:
  `skillListingBudgetFraction`, `SLASH_COMMAND_TOOL_CHAR_BUDGET`, `skillListingMaxDescChars`.
  `disableBundledSkills` turns off "the skills and workflows included with Claude Code" (scope "Any
  file", settings reference index).
- Cloud sessions: personal skills do not load; "you can instead commit the skill to the
  repository's `.claude/skills/`. Plugins declared in the repository's `.claude/settings.json` and
  plugins enabled only in your user settings don't load in cloud sessions" (skills page). A
  single-repository cloud session reads the committed `.claude/settings.json`, not
  `~/.claude/settings.json` nor `.claude/settings.local.json`
  (https://code.claude.com/docs/en/settings, "Settings in cloud sessions"). Meada's ponytail reaches
  the loop only because the environment setup script runs `claude plugin install ponytail@ponytail`
  on top of the repository's `enabledPlugins` (maintainer's loop notes; probe commit on
  `origin/claude/probe-ponytail-7csg0b`).

## 4. What Meada's listing actually costs

The 38 vendored skills split by upstream bucket (`.agents/skills-lock.json`): `engineering/` 20,
`productivity/` 7, `in-progress/` 7, `misc/` 4. Four carry local edits: `implement-spec`,
`research`, `to-tickets`, `grilling`. 23 are user-invoked and cost nothing in the listing. 15 are
model-invoked:

| Skill | Bytes | Skill | Bytes |
| :- | -: | :- | -: |
| code-review | 432 | migrate-to-shoehorn (misc) | 191 |
| wizard | 323 | diagnosing-bugs | 175 |
| codebase-design | 284 | domain-modeling | 170 |
| git-guardrails-claude-code (misc) | 273 | grilling | 164 |
| setup-pre-commit (misc) | 258 | tdd | 156 |
| research | 250 | writing-for-agents | 125 |
| scaffold-exercises (misc) | 226 | pr | 33 |
| prototype | 192 | **total** | **3,252** |

Measured listings:

| Session kind | Listing | Entries | Meada's share |
| :- | -: | -: | -: |
| Loop sessions (sandcastle runner, 6 transcripts) | 8,528 chars | 27 | 3,252 B, 15 entries |
| Maintainer, main checkout (6 transcripts) | 20,362 chars | 46 | 2,304 B, 11 entries |

The local share is smaller because `.claude/settings.local.json` sets the four `misc/` skills
`off`; that file never reaches a cloud or container session. The rest of the loop's listing is
Claude Code's bundled skills (`dataviz` 1,459 B, `claude-api` 1,092, `update-config` 707, `run`,
`loop`, `plugin-authoring`, `workflow-authoring`, `keybindings-help`, `fewer-permission-prompts`,
`simplify`, `security-review`, `init`: about 5.3 KB). The rest of the maintainer's is user-scope
skills and plugins (`anthropic-skills:*`, `ponytail:*`, `impeccable`, `the-fool` and others).

At about 4 bytes per token, Meada's share is about 800 tokens in a loop session: about 1% of the
~70k-token cloud prefix, and about 0.1M of the ~32M cache-read tokens of a ~120-call ticket.

The "44 to 46 KB" figure that prompted this note was the size of the `skill_listing` line in a
cloud session's transcript, which also carries a name and a digest per skill. A probe session in
the loop's cloud environment (2026-11-01, Sonnet, `--permission-mode auto`) split its own listing
by entry; the text that reaches the prompt is 20,542 bytes, 48 entries:

| Source | Entries | Bytes |
| :- | -: | -: |
| `anthropic-skills:*`, bundled with Claude Code on the web | 14 | 9,952 |
| Claude Code's own bundled skills (`dataviz` 1,447, `claude-api` 1,082, `update-config` 707, `run`, `loop`, `session-start-hook` and others) | 13 | 5,553 |
| Meada's model-invoked skills | 15 | 3,252 |
| `ponytail:*` (the plugin the environment's setup script installs) | 6 | 1,738 |

So Meada's own share is the same 3,252 bytes measured locally, and the 23 user-invoked skills are
absent in the cloud too.

## 5. Alternatives

Each row gives Meada's bytes in the listing of a loop session and of a local one.

| Option | Loop | Local | Cloud reach | Pinning | Local edits |
| :- | -: | -: | :- | :- | :- |
| (a) Keep all 38 | 3,252 | 2,304 | Yes, from the clone | Commit + tree per skill | Kept, marked by `tree` |
| (b1) Drop the 4 `misc/` skills | 2,304 | 2,304 | Yes | Same | Kept |
| (b2) Vendor only the loop's closure | 1,681 | 1,681 + user scope | Yes | Same | Kept |
| (c) Router skill over the rest | no gain | no gain | Yes | Same | Kept |
| (d1) Plugin for the maintainer, loop keeps its own | 1,681 | ~1,681 + ~2,500 plugin | Plugin only via setup script | Marketplace sha, auto-update | Lost in the plugin copy |
| (d2) User scope `~/.claude/skills` for interactive ones | 1,681 to 2,304 | unchanged | Personal skills never reach it | By hand | Kept |
| (e) skills.sh as AI Hero ships it | as (a) or (b) | same | Yes | Ref, not commit | Overwritten by `update` |

Notes per option:

- **(a)** Works today and in every runner. Costs the four frozen `misc/` entries (948 B) in every
  loop session, and 30 skills in the `/` menu that only the maintainer reaches for.
- **(b1)** Upstream calls `misc/` frozen and keeps it out of the plugin (section 2.2); Meada never
  invokes any of the four, and locally they are already `off`. Same effect without deleting files:
  move the four `"off"` entries from `.claude/settings.local.json` into the committed
  `.claude/settings.json` (scope "Any file", read by single-repository cloud sessions).
- **(b2)** The loop runs `/implement`, `/code-review`, `/implement-spec`, `/retro`, `/handoff`,
  `/triage`, `/to-spec`, `/to-tickets`. Their Skill-tool calls reach `tdd`, `code-review`,
  `domain-modeling`, `grilling`; other skills reach `codebase-design`, `research`, `prototype`;
  `CODING_STANDARDS.md` and `docs/agents/architecture.md` cite `pr`. That closure is 1,681 B. Going
  from (b1) to (b2) saves another 623 B (`wizard`, `diagnosing-bugs`, `writing-for-agents`) at the
  cost of moving them out of the repository; the 19 user-invoked skills dropped with them save no
  bytes at all.
- **(c)** A router fixes cognitive load, not context load (section 2.3). The user-invoked skills
  already cost zero. Folding the model-invoked ones behind one router entry would break the
  Skill-tool calls between skills, which need a model-invoked target, and the agent's autonomous
  triggering. Not worth it.
- **(d1)** The plugin ships 27 skills, 11 of them model-invoked; it would list them again under
  `mattpocock-skills:` (about 2.5 KB) in every local session, next to the vendored copies ("every
  skill twice"), unless the vendored ones go. It cannot carry the four local edits, and
  `skillOverrides` cannot trim it. In the cloud it loads only if the setup script installs it.
  This is how the maintainer's user settings already look (`mattpocock-skills@claude-plugins-official`
  enabled in `~/.claude/settings.json`, disabled in Meada's committed `.claude/settings.json`).
- **(d2)** Personal skills win over project skills of the same name and never reach cloud sessions,
  so a skill must not live in both places. Useful for hygiene (the `/` menu, other repositories),
  not for tokens: every interactive-only skill except three is user-invoked and free already.
- **(e)** AI Hero's own two routes are the plugin (d1) and skills.sh copies selected with
  `--skill`, which is what Meada does by hand. The CLI's `skills-lock.json` records a ref and a
  content hash, so `npx skills update` would overwrite the four edited skills; Meada's lock
  (upstream commit plus `upstreamTree`/`tree` per skill) is stricter on both counts.

## 6. Recommendation

Keep vendoring under `.agents/skills/` with the lock. It is the route upstream offers for editable
files, the only one that reaches the loop's cloud sessions with no setup-script work, and the only
one that keeps `implement-spec`, `research`, `to-tickets` and `grilling` as Meada edited them.

Then, in order of value:

1. **Stop listing the four `misc/` skills in loop sessions.** Either un-vendor them (upstream:
   frozen, unpromoted) or commit their `"off"` overrides to `.claude/settings.json`. Loop listing
   share: 3,252 B to 2,304 B. Whether a sandcastle or cloud run applies a committed `skillOverrides`
   was not tested; one probe session would confirm it.
2. **Leave the user-invoked skills where they are.** They cost no listing bytes. Un-vendoring the
   `in-progress/` ones nobody uses (`chief-of-staff`, `loop-me`, `writing-beats`,
   `writing-fragments`, `writing-shape`, `setup-ts-deep-modules`, `claude-handoff`) would tidy the
   `/` menu but saves nothing per call.
3. **Look at bundled skills before any further skill work.** In the loop's listing they are about
   5.3 KB of 8.5 KB. `disableBundledSkills` (any settings file) or the
   `CLAUDE_CODE_DISABLE_BUNDLED_SKILLS` environment variable named in the maintainer's notes would
   remove them; check first that no loop step relies on `/simplify`, `/security-review`, `/loop` or
   `/run`. Claude Code also bundles a `/code-review` (the skills page's `/review` alias example);
   which one wins against the project skill of the same name was not checked here.
4. **Refresh the pin separately.** Upstream `main` is 8 commits past `49dd158` and touches
   `code-review`, `implement-spec`, `research`, `grilling`, `codebase-design`,
   `improve-codebase-architecture`, `setup-matt-pocock-skills` and `wayfinder`; three of those carry
   local edits, so the refresh is a three-way merge per edited skill.

Trade-off in plain terms: the whole Matt Pocock set costs a loop session about 800 tokens per call,
about 1% of its fixed prefix. Item 1 removes about a quarter of that; nothing in this area moves
the loop's bill much. The tool schemas (223 KB in the cloud probe, Artifact alone 58.8 KB per the
maintainer's notes) and the bundled skills are where the bytes are.

## 7. Unverified

- That a committed `skillOverrides` and `disableBundledSkills` apply in the loop's cloud and
  sandcastle sessions (the docs say single-repository cloud sessions read `.claude/settings.json`;
  not probed).
- Which of the project `code-review` and the bundled `/code-review` a bare `/code-review` resolves
  to.
- aihero.dev pages other than `/skills` (`/getting-started~svvtb` returned 404; the RSS feed
  returned 404).
- The token estimate uses 4 bytes per token, not a tokenizer count.
