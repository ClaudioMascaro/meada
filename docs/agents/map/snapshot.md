# Snapshot store

Detail behind the [architecture map](../architecture.md), which keeps the flow, identities,
seams and invariants (the limits are in [Known ceilings](../ceilings.md)). The code wins over
this file, and a change to these modules updates it.

- `snapshot.rs`, `snapshot/store.rs`, `snapshot/pull.rs` (feature `snapshot`; the only places
  `object_store`, `reqwest` as a client of Meada's own and `tar` are used): the snapshot store. `File` is the
  `[snapshot]` section and `File::validate` its `Config` (an `https` endpoint, or `http` on
  loopback, with no user or path, and `path_style` for an `http`, IP or `localhost` one;
  `region`, `bucket` and `prefix` shapes; one credential source,
  the static keys only with `static`, at most one of each `*_env`/`*_file` pair). `bundle_id`
  and `generation_id` are the ids of ADR 0012 (`is_id` their shape); `entry_blocks` gives each
  manifest entry's blocks as canonical JSON (`Listed::blocks`); all four, and `fence`, are
  `meada_core::ids`'s, re-exported. `publish` is `meada snapshot
  publish`: per repository `index::open_read` plus `index::fresh`, `repo::unclean` and
  `repo::head`, every problem listed; then `store_generation` uploads the missing bundles (a
  first pass measures the tar's size and hash, a second streams it as a multipart upload with
  them as metadata, aborted if they differ), reads each back from metadata, writes the document
  and `current` last; an existing document is verified instead (`verify`) and never rewritten.
  `Document::workspace`, optional, is the id of the pulled workspace version with `source =
  "store"` (`Manifest::version`), absent with `source = "file"`, and part of no id: a stored
  generation keeps the version that first produced it, and older binaries ignore the field.
  The tar is `index.db`, then the mirror under `mirror/` in path order, symbolic links as links.
  `store::Store` is the client `meada snapshot pull` (#273) reuses: object_store's S3 client
  over `Guard`, a reqwest client with no redirect and no proxy that refuses any origin but the
  bucket's and the chosen credential source's (STS in `region`, the Pod Identity agent, IMDSv2),
  whose TLS is `store::tls` (rustls with ring and the system's roots, optionally augmented by
  the manifest-relative `snapshot.ca_file` PEM certificates, handed to a reqwest built
  with `rustls-no-provider`; object_store signs with ring too). Ring is the only rustls provider
  in any build (ureq brings it, and redis reads it as the process default): a second one, as
  object_store's `aws` feature would add with aws-lc-rs, makes rustls panic on the first TLS
  connection of a build with both modules;
  credentials come only from that source (static ones built from the variables or files, the
  others through object_store's providers given exactly those inputs, IMDSv2 over IPv4 only),
  stay in memory and are scrubbed from every error: the static secret is replaced, and an S3
  error body loses its `CanonicalRequest`, `StringToSign` and their `*Bytes` echoes. The
  secret files are read through `modules::read_secret`, as `valkey.rs` reads its own. Each
  bundle keeps its `open_read` connection, the read transaction its freshness check ran in,
  until its upload ends. Every object carries `meada-size` and `meada-blake3` metadata.
  `snapshot/pull.rs` is `meada snapshot pull` (`pull`, once; `main.rs` loops for `--every`),
  which treats everything it reads from the store as hostile. Under `local_dir`: `current`
  (the node pointer file, one line `<generation id> <fence>\n`, fence `0` from the store's
  pointer and the fence read with the id from Valkey with `[generations]`; `pull::current`
  reads it and checks the id and the fence), `pull.lock` (an advisory lock taken with
  `try_lock`: a second pull returns `Pulled::Running`), and per held generation `<generation
  id>/` with `generation.json` (the document as stored) and per repository `<bundle
  id>/index.db` and `<bundle id>/mirror/`; a pull in progress writes only `<generation
  id>.partial/` and `current.new`. Steps: the store's `current` must be an id and a newline
  (`store_pointer`); `checked` refuses a document before any path is built from it (its
  generation id, a `workspace` field that is a version id when present
  (`Msg::SnapshotBadWorkspace`), `identity_problem` (`meada_core::manifest`) per identity plus no duplicate and no character
  a git remote URL cannot hold, each bundle key `repos/<identity>/<bundle id>.tar` under the
  prefix, hash and `HEAD` shapes, the size sum, the generation id derived again); per bundle,
  `Store::download` streams it to a file checked against the document's size and hash, and
  `extract` checks every entry itself (no `unpack`: `index.db` first, then only `mirror/`; only
  normal components; regular files created `create_new`, `0644` or `0755`; directories made
  without following a link; symbolic links only to relative targets that fold inside the
  mirror, and after the last entry none through another link; hard links only to an earlier
  regular file of the mirror; no nested repository, that is no `.git` component, in any case,
  below the mirror's own `.git` and nothing under its `.git/modules`, since git runs a
  submodule's own configuration when `git status` descends into it; any other type refused);
  `index::vet` requires the extracted `index.db` to hold exactly the schema objects this build
  creates (type, name, table and SQL against a fresh in-memory index: no view, trigger, table
  or index more, none altered) and its header to name the rollback journal (bytes 18 and 19
  both 1, so no read-only open makes `-wal` or `-shm` files), naming the repository and the
  object;
  `settle` replaces `.git/config` with `[core]` and the identity's `origin`, removes `hooks`,
  `commondir` and `objects/info/alternates`, and checks the checkout, identity, `HEAD` and a
  clean `git status` that does not descend into submodules (`repo::unclean(_, false)`; the
  publish check on the operator's own mirrors keeps descending). `repo::isolate(local_dir)`
  makes every `git` call under it run with `GIT_CONFIG_NOSYSTEM=1`, `GIT_CONFIG_GLOBAL` set to
  the null device and `-c core.fsmonitor=false`. Then the partial
  directory is renamed to `<generation id>/`, `current.new` is written, synced and renamed over
  `current`, and every id-named directory but the new current and the one just replaced is
  deleted. A failure removes the partial directory and leaves the pointer and every held
  generation as they were.
  `snapshot/serve.rs` is what serving reads from a node (`--generations`): `node` (the
  manifest's `[snapshot]` section and `local_dir`), `held` (the current generation: the
  pointer file through `pull::current`, then `generation.json`, bounded and checked as the
  generation's own), `Current` (the last generation `held` read, kept while the pointer names
  it; `Current::pinned` keeps one `Held` per pinned id, read once while held and dropped once
  `ids` no longer lists it, so what it keeps is bounded by what the node holds; a read that fails
  because a pull deleted the directory after the listing is `None`, the pin's 404, through
  `unless_gone`), `ids` (the generations held) and `reach` (the store's current id within `CURRENT_ID_TIMEOUT`, two
  seconds, for the probe; with `[generations]`, `reach_pointer`: Valkey's current id, then a
  HEAD of its document in the store, within the same bound, the reason prefixed `valkey:` or
  `store:`). It only reads; the pull is the only writer of `local_dir`. A pinned call still open
  on a generation that a pull's retention deletes may fail mid-call: the pull does not wait for
  what the server is still reading.
  `snapshot/generations.rs` (feature `generation-pointer`, which implies `snapshot` and
  `valkey`; no crate of its own) is the generation pointer, section `[generations]` (`keep`,
  default 3, at least 2; `Manifest::read` refuses it without `[snapshot]` and `[valkey]` through
  `modules::needs`). It owns the Valkey keys, both under the `[valkey]` prefix's hash tag:
  `{<key_prefix>}:current`, a hash `{id, fence}`, `{<key_prefix>}:retained`, a list of
  `<id>:<fence>` newest first, each id once, and `{<key_prefix>}:kept` (`kept`), the set of ids
  an operator keeps by hand (`meada snapshot pointer retain`), at most `KEPT_MAX`. `point` runs
  the one script that writes the pointer (`EVAL`; KEYS current, retained, `{<key_prefix>}:lock`
  and kept, ARGV id, fence, keep and the refresh lock's token, empty without one): with a token it returns 0 unless the lock
  holds it (`Msg::LockNotHeld`) and -1 unless the fence is above `current`'s
  (`Msg::FenceRefused`, naming `{<key_prefix>}:fence` and `meada snapshot fence recover`, as
  `Msg::FenceBelowNode` and `Msg::FenceStuck` do with `--at-least`; the two fences compare
  as decimal text, leading zeros dropped, so none loses precision as a Lua number, and -2 for a
  stored fence that is no decimal); then it removes every entry of the id, pushes
  `<id>:<fence>`, trims (the oldest entries whose id is not in kept go until at most `keep` of
  those remain; a kept entry never goes) and sets the hash. `publish` connects to Valkey before any upload and
  calls `point` last, with fence `0` and no token without `[refresh_lock]`, instead of writing
  the store's `current`; `meada snapshot pointer set` (`operation::pointer_set`) calls it with
  its own token and fence to move the pointer to a retained generation, a rollback that the
  next publish undoes. `read` is the pull's side: `HMGET current id fence` and the whole
  retained list (`LRANGE retained 0 keep+KEPT_MAX`), every id checked by `is_id` and every fence
  by `snapshot::fence` before anything else; a missing pointer, an empty list, a list longer
  than `keep` plus `KEPT_MAX` (`Msg::GenerationsTooLong`) or a malformed entry is an error
  naming the key. With
  it, `pull_locked` (through `Source::wanted`) fetches the current generation if the node lacks
  it and swaps the pointer file to `<id> <fence>` as read (a failure there fails the pull and
  changes nothing), then fetches every retained one the node lacks, each failure a warning
  (`Msg::SnapshotRetainedSkipped`, naming the id and the cause, on stderr) that blocks nothing
  and that the next pull retries, then deletes every held id outside the current, the retained
  and the one just replaced. A pull that re-fetched the current generation (its directory gone)
  reports `Pulled::Swapped`, so `--every` prints its id. Before any fetch, a current generation
  whose fence is below the one in the node pointer file fails the pull with
  `Msg::FenceBelowNode` and changes nothing; with no pointer file the node takes it whatever
  its fence. `engine::status_generations` adds a `fence: stuck: ...` line (`Msg::FenceStuck`)
  when the node's fence is above the one `reach_pointer` read, with no effect on the exit code.
  `snapshot/heartbeat.rs` (feature `generation-pointer`, crate `nix` for the host name; ADR
  0014) is the node heartbeat. `main.rs`'s `pull` loop calls `heartbeat::beat` at the end of
  every round of `pull --every` (never of a single pull), whether the round swapped, changed
  nothing or failed, and `heartbeat::check` once before the loop: with `[generations]`, a host
  name that is no node name, with no `[snapshot] node_name`, refuses to start
  (`Msg::NodeNameHost`, naming `[snapshot] node_name`). Without `[generations]` neither runs
  and no connection opens. The key `{<key_prefix>}:node:<name>` holds a JSON object (`name`;
  `generation` and `fence`, from the node pointer file, null without one; `held`, `serve::ids`;
  `ready`, `engine::served_generation` succeeding, the predicate `status --generations` exits
  on; `version`; `time`, RFC 3339 UTC to the second), and the `WRITE` script sets it with a TTL
  of three `--every` periods, adds the name to `{<key_prefix>}:nodes` scored by the expiry in
  Unix seconds by Valkey's own clock (`TIME`), and removes every name whose score is at or
  below now, so the set never keeps the names of pods long gone and the console lists nodes
  without scanning. `[snapshot] node_name` (`snapshot::is_node_name`: lowercase letters,
  digits, `.` and `-`, 1 to 63 characters; an error naming the file, the line and the key
  otherwise) defaults to the host name in lowercase; names must be unique, as pod names are,
  since two nodes with one name overwrite each other's heartbeat. A write that fails, or that
  Valkey does not answer within `WRITE_TIMEOUT` (two seconds, the bound the probe waits for
  Valkey), is one stderr warning for the round (`Msg::HeartbeatNotWritten`, the key and the
  scrubbed reason): the round's outcome, the pointer file, the exit code and the readiness do
  not change. `heartbeat::read`, shared by recovery and the console, selects live names with
  Valkey `TIME`, reads their keys and applies the complete `heartbeat::checked` validator.
  Malformed names never form keys and malformed documents never contribute fences;
  `Malformed` supplies the CLI warnings and console rows.
  `snapshot/versions.rs` (feature `generation-pointer`, ADR 0014) holds the workspace
  versions: the `[[repository]]` entries of a workspace as immutable documents in the store, at
  `workspaces/<version id>.json` under `[snapshot] prefix` (`Version`: `id`, `author`, `time`
  in RFC 3339 UTC, `message`, and `repositories` in manifest order, each entry its `repo` and
  `path` as written, its `owner` when it has one, plus its blocks as `entry_blocks` gives
  them), stored with the size and hash metadata every object carries and never rewritten. The
  id is `meada_core::ids::workspace_version_id` of the entries alone, owners included.
  `meada index`, `status`, `export` and `snapshot publish` ignore the owner and the `added_by` of a pulled version
  (the role check reads the `added_by`, and under `[oidc.github]` they leave out what its
  exclusions file names),
  and a generation document's entry blocks holding an `owner` or an `added_by` are refused (`snapshot/pull.rs`). The current version is
  `{<key_prefix>}:workspace`, a hash `{id, fence}` under the pointer's hash tag (`pointer_key`,
  read by `current`, moved by `meada workspace apply`, #302). `import` (`meada workspace import
  <file> --message <text> [--author <name>] --workspace <bootstrap manifest>`) reads the file
  once (`read_import`, bounded by `VERSION_MAX`; the CLI audits the id of that same text, and a
  file it cannot read writes no audit record, #450), takes its `[[repository]]` entries through
  `meada_core::manifest::validate_import`, names each other
  top-level key on stderr as not imported (`Msg::WorkspaceNotImported`), and validates before
  `Store::connect`; entries whose document is stored upload nothing and answer it with its first
  author and message (`Msg::WorkspaceStored`). Shared `store_version` checks the serialized
  document against `VERSION_MAX`, then `Store::create` uses conditional create with size/hash
  metadata: concurrent imports preserve the first author, time and message, and a losing
  writer reads the checked original. HTTP `save` shares the preparation and storage under the
  refresh lock; CLI `import` requires only `generation-pointer` and takes no refresh lock.
  `list` reads and checks the pointer first (a malformed one, `Msg::WorkspaceBadPointer`, refuses the listing), then every object under
  `workspaces/` (`Store::list`), sorted newest first by time, then id; a name that is not an id,
  or a document that fails `checked`, is a `!` line. `show [<version>]` checks the argument
  first (`Msg::BadVersionArg`), takes the current version without one
  (`Msg::WorkspaceNoCurrent`, naming `meada workspace apply`) and prints the entries as
  `[[repository]]` tables, which `import` reads back to the same id. `checked` treats a
  document as hostile: at most `VERSION_MAX`, its id the one its key names, its time's shape,
  its entries rendered as `[[repository]]` text and validated as a workspace's
  (`meada_core::manifest::validate`), and the id derived from them again. The commands need
  `[generations]` in the bootstrap manifest (`Msg::WorkspaceNoSections`), not `source =
  "store"`, so an operator imports before switching. The author without `--author` is
  `cli_author`, `cli:<login>@<host>` (from the environment in a build without `refresh-lock`).
  `apply` (`meada workspace apply <version>`, feature `refresh-lock`) checks the id first
  (`Msg::BadVersionArg`), needs `[refresh_lock]` (`operation::sections`), reads and checks the
  version's document in the store (missing, `Msg::WorkspaceNoVersion` naming its key), then runs
  under `operation::locked` (holder `workspace apply <author>`), for a subject other than the
  global operator (`Acting`: its `Role`, groups, `Grant`, login and GitHub host), the
  authorization against the version then current: its role recomputed (`authorize::role`; none
  current, or no team left, is `ApplyError::RoleLost`), then `authorize_entries` over the whole
  current entries and the applied ones, so an addition on the GitHub host without `added_by` is
  `unattributed` (`ApplyError::Refused` with the refusals; `Msg::WorkspaceRefused` counts them).
  Either releases the lock and moves nothing; two applies serialize on the lock, so a version is
  checked against the one it replaces. The CLI passes the global operator, which reads nothing
  more; the console passes the session's role (#324). `apply` returns `Applied`: the line and
  the owner groups of the entries it touched. Then the `APPLY` script: it sets
  `{<key_prefix>}:workspace` to `{id, fence}` only while the lock holds the token and the
  operation's fence is above the stored one (decimal text, as `POINT` compares;
  `Msg::FenceRefused` otherwise); it prints `<id> <fence>`. Rolling back is applying an older
  version. `pull` (`meada workspace pull`) reads the pointer (`current`; none is
  `Msg::WorkspaceNoCurrent`, naming `meada workspace apply`) and the document (at most
  `VERSION_MAX`, `checked`) before anything on disk changes, then, under `<index_dir>/workspace/`
  (`DIR`) and its `pull.lock` (an advisory lock, one pull at a time), writes `<id>.json` (the
  document as stored, through `pull::replace`: `<id>.json.new` synced, then renamed; the helper `swap`, `write_contexts` and the exclusions file share) and swaps `current` (one line `<id>
  <fence>`, `pull::swap`); a pointer whose fence is below the current file's, or equal with
  another id, is `Msg::WorkspaceFenceBelow` naming both fences and the file, and the same id and
  fence is `Pulled::Current` (the id on stdout, `Msg::WorkspaceAlreadyCurrent` on stderr). Only
  the current version's document and the one it replaced stay; a failure leaves the current
  file and the documents as they were. `pulled` reads the version the current file names from
  disk (`pull::pointer_file`, `Msg::WorkspaceCurrentBad` for a malformed one), bounded and
  checked as a pull checks it, and opens no connection: `Manifest::read_listed` uses it.
  The refresh job of a managed workspace runs, under `meada snapshot lock`, `meada workspace
  pull`, the fetch of the mirrors `meada workspace show` lists, `meada index --workspace` and
  `meada snapshot publish`: `apply` needs the lock, so it cannot run between the pull and the
  publish, and the whole refresh sees one version.
  `snapshot/lock.rs` (feature `refresh-lock`, which implies `generation-pointer`; crate `nix`,
  Unix only, a `compile_error!` elsewhere) is the refresh lock, section `[refresh_lock]` (`ttl_seconds`, default 300, at
  least 10; `Manifest::read` refuses it without `[generations]`). Keys under the same hash tag:
  `{<key_prefix>}:lock`, `{<key_prefix>}:fence`, which Meada only `INCR`s and `fence recover`
  raises, never lowers, and `{<key_prefix>}:holder`, a hash beside the lock with its TTL
  (`host`, the lowercased host name; `pid`; `started`, RFC
  3339 in UTC to the second; `holds`: `lock <program>`, the child's program without its
  arguments, or `<operation> cli:<login>@<host>`). `acquire` is shared by `lock` and the
  operations: `TAKE` (`SET lock <token> NX PX <ttl>` and, when taken, the holder written with
  the same TTL, in one script; else the holder's fields, so a lock already held is
  `Msg::LockHeld` naming its host, process, start time and what holds it, or an unknown holder
  for a lock without one, every field escaped; a token of 16 bytes of `/dev/urandom` in hex; a
  script that errs releases the token anyway, since the server may have taken it), then
  `INCREMENT`: under the same token, compare the counter as decimal text against the caller's
  ceiling and issue native `INCR` once, returning `GET` to preserve precision beyond 2^53.
  The signed INCR ceiling is `LAST_FENCE` (9223372036854775807); recovery reserves one more
  acquisition for publishing, so its own acquisition cannot exceed 9223372036854775806.
  A ceiling refusal is `Msg::FenceLimit`; a result that is not a positive `u64` is
  `Msg::LockBadFence`. Wire values and recorded fences remain full `u64`. `RENEW` extends the
  holder with the lock and `RELEASE` deletes both, each only while the lock holds the token.
  `run` is `meada snapshot lock`: `acquire`, then the child, spawned on the main thread in a
  process group of its own (`process_group(0)`) with `MEADA_REFRESH_LOCK` and
  `MEADA_REFRESH_FENCE`; on Linux its `pre_exec` arms `PR_SET_PDEATHSIG` (SIGTERM) and refuses
  to run if `lock` already died, the crate's one `unsafe`. A thread waits for the child; the
  `select!` loop renews every `ttl/3` (`RENEW`, compare-and-extend, each try bounded by the next
  deadline and polled as a branch of its own, so no signal or exit waits for Valkey), counts a renewal from the instant before it was sent, and calls the lock lost when
  a renewal finds another token or none succeeded for a whole TTL. Lost, or a SIGTERM, SIGINT,
  SIGHUP or SIGQUIT to `lock`: SIGTERM to the group, SIGKILL after `KILL_GRACE` (10 s). Once the child exits, the
  group gets SIGKILL (nothing is left in it; a grandchild that calls `setsid` escapes), `RELEASE` (compare-and-delete) runs within `RELEASE_TIMEOUT`,
  and `run` returns the child's code (128 plus the signal) or `Msg::LockLost`. `from_env` is
  `publish`'s side: with `[refresh_lock]` it needs both variables (`Msg::LockEnv`), `holds` (`GET
  lock` equals the token) runs before each bundle upload and before the document's (a token
  with no `[generations]` is an error, never a pass), and the
  token and fence go to `point`.
  `snapshot/operation.rs` (feature `refresh-lock`, ADR 0014) holds the operations, which the
  console calls in process too: each checks its arguments first (an id by `is_id`, a fence by
  `snapshot::fence`), needs `[refresh_lock]` (`Msg::LockNoSection`), runs under `acquire` with
  a holder naming it and its author, makes one write through a script that checks the token,
  and calls `release` whatever the write did, so it leaves no lock and no holder (a release
  Valkey misses expires with the TTL). `pointer_set` (`meada snapshot pointer set <generation>`)
  refuses an id outside `retained` (`Msg::PointerNotRetained`), then
  calls `point` with its token and fence, so the moved pointer's fence is above every node's
  and nodes swap on their next pull; it prints `<id> <fence>`. The rollback holds until the
  next publish moves the pointer past it: to stay back, the operator suspends the refresh job.
  `pointer_retain` (`meada snapshot pointer retain <generation>`) first reads the generation's
  document from the store and checks it as a pull does (`pull::stored_document`; missing or
  malformed, the error names its key, before the lock), then `RETAIN`: an id already kept
  changes nothing, a kept set already holding `KEPT_MAX` is `Msg::KeptFull`, an id with no
  entry is appended at the list's oldest end as `<id>:<fence>` with the operation's fence
  (which `fence recover` counts), and the id joins kept; it prints the id. `pointer_drop`
  (`meada snapshot pointer drop <generation>`) runs `DROP`: the current id is
  `Msg::DropCurrent` naming `current`, one with no entry `Msg::DropNotRetained` naming
  `retained`; else its entries leave the list and the id leaves kept, and the next pull deletes
  it; it prints the id.
  `fence_recover` (`meada snapshot fence recover [--at-least <fence>]`) reads the pointer, the
  workspace pointer (`versions::current`: `apply` takes its fence from the same counter), every
  `retained` entry and the heartbeat (`{<key_prefix>}:node:<name>`, its `fence` a `u64` or
  null) of every name of `{<key_prefix>}:nodes` scored above Valkey's `TIME`, as hostile input: a malformed
  pointer, entry, node name or heartbeat is a `Msg::FenceNotCounted` line on stderr and does not
  count, a name whose key expired is skipped. The floor and recorded maximum are checked
  against `LAST_FENCE - 1` before acquisition; records are read again under the lock to cover
  a publisher between the preflight and acquisition. `RAISE` checks the token and ceiling and
  sets `fence` to the highest of those and the floor only when above the stored one (decimal
  text, as `POINT` compares); it prints the fence it leaves, including its own `INCR`.
  Recovery refuses a counter already at that ceiling without consuming the last acquisition;
  it never lowers or clamps fences. The next locked publish can still use `LAST_FENCE`.
