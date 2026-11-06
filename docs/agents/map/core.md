# The core crate (`meada-core`)

Detail behind the [architecture map](../architecture.md), which keeps the flow, identities,
seams and invariants (the limits are in [Known ceilings](../ceilings.md)). The code wins over
this file, and a change to these modules updates it.

The crates are described in the [architecture map](../architecture.md#crates). The modules of
`meada-core`:

- `meada-core/src/manifest.rs`: a workspace manifest's `[[repository]]` (`EntryFile`,
  `EntryPeer`) and the `[serve]` and `[[resource]]` shapes `meada.toml` shares (`Serve`,
  `Resource`, `Role`, `declared`, `gateway_path`); `parse` and `error_at`, the errors naming
  the file, the line and the dotted key, never the value written there (`line_of`); `identity_problem` and `entry_problems`
  (an identity listed twice); `validate`, a workspace text's entries or its problems as the CLI
  prints them (a text holding only `[[repository]]` entries). `EntryFile::owner` is an entry's
  optional `owner` (ADR 0015), checked where the entry is read (`owner_problem`: one string, not
  empty, at most `OWNER_MAX` characters, no control character, no whitespace at either end;
  compared exactly), so the browser's validation, every import, every pull and every read of a
  stored version check it alike; `authorize` reads it to decide a write (#323).
  `EntryFile::added_by` is the optional GitHub login that added the entry (#330), checked there
  too (`added_by_problem`: one string, 1 to 39 characters of lowercase ASCII letters, digits,
  `-` and `_`; compared exactly); attribution and no grant. Under the GitHub grant provider
  `authorize` reads it to decide a write (#333) and `meada github-app roles` excludes an addition
  whose adder holds no accepted role (#331). `EntryFile::added_by_time` is when the console wrote
  the `added_by` (#454, `is_time`: one string `YYYY-MM-DDTHH:MM:SSZ`), which the role check
  compares with the first permissions set's start.
  `validate_import(file, src)`
  validates entries while ignoring other top-level keys, with CLI import's file prefix on
  identity errors; CLI, HTTP and WASM import validation share it, preserving empty workspaces and ignored
  bootstrap sections.
- `meada-core/src/ids.rs`: `bundle_id`, `generation_id` (ADR 0012), `workspace_version_id`
  (ADR 0014: the canonical JSON of the entries, `{"blocks","path","repo"}` in manifest order, with
  `"owner"` between `blocks` and `path` and, keys being sorted, `"added_by"` (#330) before
  `blocks` and `"added_by_time"` (#454) after it, each only when the entry has one, so an entry
  without them keeps its text and its version id), `is_id`, `fence`, `entry_blocks` (every key of
  an entry but `NON_BLOCK_KEYS`: `repo`, `path`, `owner`, `added_by` and `added_by_time`; none is
  a block, so no bundle id reads them).
- `meada-core/src/workspace.rs`: `diff(file, before, after)` validates through `validate_import`
  and lists added/removed/changed identities and changed dotted keys in manifest order. Array
  tables retain each field's row position and absence; common entries reordered relative to
  one another report the `repository` array key. Formatting is ignored. `entries_text` is the
  pure JSON-repositories-to-TOML conversion shared with `snapshot::versions` and WASM; it prints
  an entry's `owner`, `added_by` and `added_by_time` right after its `path`, before the block tables, so the text reads back to
  the same id. `version_entries` (`entriesJson`) is the opposite way, a text to the entries a
  version document holds, so the browser authorizes and composes the same entries the server
  stores: `entry`, which builds one, is the function `snapshot::versions` stores them with. An owner change alone is the
  changed dotted key `repository.owner`, likewise `repository.added_by`.
- `meada-core/src/authorize.rs` (ADR 0015): what a subject may change in a workspace version,
  over a version document's JSON `repositories`. `authorize_entries(role, groups, grant, login,
  github_host, current, new)` returns the refused entries (`Refusal`: `identity`, `change`
  `add`/`edit`/`remove`, `reason`), the new entries' order then the removed ones in the current
  order; the global operator (`Role::Operator`) gets none. `login` and `github_host` are the
  subject's GitHub login and the GitHub grant provider's host (ADR 0016, #333), both none
  without `[oidc.github]`, where every answer is ADR 0015's. For anyone else, entries pair by `repo`,
  one is changed when anything but its position differs, and each changed entry gets the first
  rule it fails: `outside_grant` (under the GitHub grant provider, never for an addition on its host,
  `on_host`: the identity's first segment), `unattributed` (under the GitHub grant provider, an addition on
  its host without `added_by`, or by a subject without a login), `ownerless` (the current entry
  has no owner, or the addition has none), `owner_change` (an edit's owner differs, cleared
  included), `added_by_change` (under the GitHub grant provider, an edit's `added_by` differs: changed,
  cleared or set), `not_your_team` (the owner is none of its teams: its groups owning an entry
  of `current` the `Grant` reads). `role` is admission's rule (#324): the global operator stays
  one, anyone else is a team operator while it has a team, else a viewer.
  `compose_entries(grant, login, github_host, now, current, submitted)` is the version a team
  operator's save stores: the current entries in place, those the grant reads replaced by their
  submitted form or dropped, those outside it kept, additions appended in submitted order; under
  the GitHub grant provider each addition is attributed (`added_by` the login on its host whatever the text
  held, absent without a valid login; absent off it). Every entry it writes gets its
  `added_by_time` from `now`, never from the text (#454): the current entry's while the
  `added_by` is unchanged, `now` for another one under the GitHub grant provider, none without
  the provider or without an `added_by`. The attribution step comes before
  authorization: the server and the browser both authorize the change from the entries the
  grant reads to `compose_entries` over them, so a forged `added_by` decides no reason. The
  binary's save and apply and the browser (`authorizeEntries`, `composeEntries` and `touchedOwners` in `meada-core/src/wasm.rs`,
  `login` and `github_host` as `string | undefined`, `composeEntries` with `now` the browser's
  time) call the same functions.
- `meada-core/src/msg.rs`: the messages of those checks and the language setting the binary's
  `msg.rs` reads too; `meada-core/src/http.rs`: `normalize_path` and `PARAM`, which the HTTP
  kind re-exports. `qualify` and `parent` (dotted names) live in its `meada-core/src/lib.rs`.
- Provisioning export (#656): `meada_core::export::IacNode`, `meada_core::export::Provisioner`
  and `meada_core::export::Provision` carry the optional format-1 arrays `iac_nodes`,
  `provisioners` and `provisions`. Empty arrays are omitted; node identity and root label
  distinguish module instances, and provisioning links keep their own level and reason.
- `meada-core/src/export.rs`: the export's types as `meada export` writes them (`Export` and
  every piece, `Level`, `Link`, `Process` with its `Step`, `Via` and `Mark`, `Stale`, `Cause`,
  `EntryKind`, `Format`, `How`), `Serialize` and `Deserialize`, so a parsed export written again
  (`Export::write`) is the same bytes; `parse` reads one or names the field that does not fit
  (`calls[3].span: missing field ...`), which `parseExport` exports to the browser. The binary
  re-exports the shared pieces where they lived (`index`, `contract`, `package`, `decisions`,
  `community`, `entry`, `document`) and keeps its own `Export`, `Provider`, `Consumer`,
  `Document` and `Package` with what only the build needs, turned into these by
  `index::Export::written`. Its structs, with `Shape` and `Drift`, are `#[non_exhaustive]`, so a
  new field is a minor change for crate users, the binary included: one whose fields all have a
  default derives `Default` and is built with `T::default()` and field assignments; one with a
  field that has none has `new` taking every field in declaration order, a later field starting
  empty and set after it (`Shape::new` takes the source alone).
- `meada-core/src/shape.rs`: shapes and shape drift (ADR 0009: `Drift`, `{missing, unchecked}`,
  `shape::drift`), which the binary reaches as `crate::shape`. `drift.rs`: how `context`,
  `search` and `status` show a link's drift (`consumers`, `matches`, `counts`, over the `Entry`s
  the binary's `drift_view.rs` places).
- `meada-core/src/render.rs`: the token budget and line helpers every answer shares (`budgeted`,
  `cut`, `note`, `tokens`, `label`, `callee`, `mark`, `rank`, `section`); `search.rs` (`Hit`,
  `answer`), `context.rs` (`Symbol::answer`, its cut order and `processes_line`, `shape_lines`)
  and `trace.rs` (`Row`, `Chain::answer` with the footer): the results of `search`, `context`
  and `trace` and their text answers. The engine walks the indexes and places each line (a
  `path:line` reads the file); these assemble and cut the answer.

- Config use export (#661): `Use` and the optional `Export::uses` keep contract,
  repository-level site and the `at` key locator, without scalar text. These are directionless sides,
  independent of `providers`, `consumers` and `links`; provisioning reads them as uses.
