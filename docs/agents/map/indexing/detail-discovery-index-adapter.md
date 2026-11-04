# Detail: discovery, the index, adapter

- `workspace.rs`, `config.rs`: `Workspace::discover` finds `Repository { root, identity, config }`
  from the `meada.toml` files (peers by `path` hint, then sibling clones; siblings that name a
  repository already in join, transitively; a sibling with an invalid `meada.toml` or
  `meada.decisions.toml` is skipped). Peers not found go to `Workspace::missing`. `config` parses
  (the `[serve]` and `[[resource]]` shapes and their deserializers are `meada_core::manifest`'s,
  re-exported)
  `[[peer]]`, `[serve] hosts`, `[serve] prefix` and `strip` (each must start with `/`, normalized
  by `gateway_key`), `[serve] documents` (`repo_paths`: repository-relative, none absolute or
  with a `..` segment, `./` and doubled `/` dropped; added to the documents discovery finds;
  `Serve::declared` is set when the `[serve]` table is present, even empty or with only
  `documents`, which makes the repository's documents provider sources) and `[[resource]]`
  (`kind` checked as a lowercase identifier, `id` not empty,
  `role` read, write or read-write, optional `via`) with `deny_unknown_fields`. `config::gateway_path(prefix,
  strip, path) -> Option<(gateway path, reason)>` is the one gateway rule (spec #19); `Serve::
  gateway_path(path)` applies it with `[serve]` prefix and strip, `Http::declare` with a
  document's server path as the prefix. `config::read<T>(root, name)` is the shared TOML reader (file, line and key in
  every error), also used for the decisions file; `Repository` carries both `config` and
  `decisions`, loaded together at discovery.
- `repo.rs`: root, identity (normalized `origin`, else `local/<dir>`), `uncommitted()`, `head`,
  `unclean` (status with ignored files, for snapshots and server-mode recording) and `changed`
  (status without them, for a local export's `dirty`), all by shelling out to `git`.
- `entry.rs`: entry points (spec #31): `find(&Local)` reads what `refresh` has at its end (the
  symbols, the targets of every call, the `exports` rows, the providers and consumers the code gives, the
  `Facts::programs` the packs set, and the console scripts of the `manifests` rows through
  the package seam's `programs`, and `Layout::programs`, Rust's binary roots) and
  returns the repository's `EntryPoint`s (identity, `EntryKind`, label, `site`), which
  `refresh` rewrites into `entry_points` and `export` loads into `Export::entry_points`.
  It imports no kind module: which side of a contract is a door, of which `EntryKind`, and how it
  is labelled are `ContractKind::entry` and `label`, pure functions of the contract since entry
  points are stored (an answer that changes needs a reindex). The index writer reads the
  `package` manifests and tsconfigs directly (`package::read_manifest` and the others it calls
  stay `pub`), and hands the kinds what it read (`Indexed`); the kinds' query-time questions
  (`declare`, `workspace_step`, `link`, `after_links`) never run at index time.
- `index.rs`: SQLite schema, `refresh`, `export` and the export types. Freshness is a pipeline in
  `refresh`: stat (mtime/size, trusted only if older than `RACY` = 2 s), hash with BLAKE3, parse
  only changed files (or those whose `settings` line moved, under the rest of their stored
  setting), parse again each file whose setting `LanguagePack::extend` now makes differ
  (`settle`, once: a kept file so parsed joins the changed ones), resolve in memory the calls of every file of a group not kept (below), rewrite rows only when a file's
  `resolved` digest changed, re-extract contracts for every group not kept (one row per side, the
  `Found::shape`s of a side reached several ways merged, as JSON in `contracts.shape`, which `export` loads into `Provider::shape` and
  `Consumer::shape`). The extraction reads values the index does not keep, so it parses again
  the files whose values the last extraction read (the `valued` table, `Code::read`) before its
  first pass, each through the values cache (`index/values.rs`, `values.db` beside the index, ADR
  0027): an entry keyed by the file's bytes, setting, pack and Meada's build gives back what the
  parse would, so of an unchanged file nothing is parsed, and each extraction writes what it
  parsed and drops the entries of the files no longer in `valued`; then each file a kind misses (`Code::misses`) until none does; a pack that missed
  nothing in a pass is done and is not extracted again (#391). A group of packs that resolve
  together (`lang::resolver_of`) none of whose files was changed, deleted or stamped by a join is
  kept when `meta.configs` (which folds Meada's version in), the member files and the stored
  `exports` match what this refresh found (`unchanged_groups`): its files are neither re-parsed
  for values, extracted nor resolved, its `contracts`, `symbols` and `calls` rows stay, and the
  entry points read its sides and reach from them (`stored_reach`). That rests on a group's
  extraction reading only its own files and finding contracts only in them (debug assertions
  in `refresh`); `Names::without` marks a question about an inline definition of a kept file,
  which makes the refresh start over keeping nothing (`refresh_keeping`). `refresh(db, root,
  identity, serve)` first brings the `documents` table (path, hash, mtime, size, `summary`) up
  to date the same way (`refresh_documents`, sharing `stat`), before the early exit and apart
  from the code rows: a document edit re-extracts no code. Without `serve.declared` the
  repository has no documents and stored rows are deleted. The walk yields every file the
  ignore rules keep; `lang::pack_for` and `document::is_document` split it, and
  `documents_found` adds each `[serve] documents` path that is a file, stat'ed outside the walk
  so `.gitignore` does not hide it; a listed path that is not a file, or that has a symbolic
  link in any segment (`plain_file`; the walk follows none either), has no row. `export` loads
  `Export::documents` (`Document {path, summary}`, path order). `refresh_stamped(tx, table, ...)`
  is the shared pass behind `documents` and `manifests` (spec #22), reading each file up to one
  byte past its reader's size limit (`read_up_to`): the `manifests` table (path,
  hash, mtime, size, `summary` as `package::Summary` JSON) holds every `package.json`,
  `pyproject.toml` and `requirements*.txt` the walk finds (`manifests_found`, through
  `package::is_manifest`; `package::read_manifest` reads each by its ecosystem; any path with a
  `node_modules` segment is skipped, since the walk
  yields it unless `.gitignore` lists it), a nameless one too, whatever `serve.declared` says;
  `export` loads them as `Export::manifests` (`serde(skip)`), and the `tsconfig.json` rows
  (`manifests_found` takes them too, `package::read_tsconfig`) as `Export::tsconfigs`. With the contracts, `refresh`
  rewrites the `exports` table (`Resolver::exported` of every pack: per file, each exported
  name with the `file` and `target` definition, constant or type it names, `target` NULL for a whole file and
  `file` NULL for nothing the repository holds, and each `export * from` a repository file as a
  row with `name` NULL), which `export` loads as `Export::exported`, the `families` table
  (`Resolver::ancestries`: each public class of a pack whose overloads merge across its family,
  Java, with its ancestors in method resolution order), loaded as `Export::ancestries`, and the
  `import` rows of `contracts`, loaded as `Export::import_calls` (all `serde(skip)`). Before extracting,
  `refresh` builds `Indexed` once (the `manifests` and tsconfigs as `read_manifests` returns
  them, the `exports` rows of every pack and the files), and the extraction loop runs only
  `contract::KINDS`, each `extract` taking the pack's `Code` and `Indexed`: the package kind
  builds the packages' public surface from it and adds the exported shapes, whose `shape` rows
  `export` loads as `Export::shapes` (`serde(skip)`, issue #175). Every `manifests` row is folded
  into the configs digest, so an edited `package.json` or `tsconfig.json` extracts again. `sort_providers` and
  `sort_consumers` put the lists back in `export`'s order after a kind's `declare` added sides.
- `community.rs`: communities (spec #31). Last in `refresh`, after the code rows,
  `write_communities` rewrites the `communities` table (name, member) from the `calls` rows at
  `exact` or `inferred`: each symbol in one is a node (a top-level caller is a file, no node),
  so an unchanged repository keeps its rows through the early exit and a
  changed one recomputes them from scratch. `communities(calls, dir)` builds the undirected
  graph (a call site adds 1 to the edge between two distinct symbols, a call to self none, so a
  node with no edge is a singleton community) and runs Leiden as spec #31 fixes
  it, in `i128` only: `local_moving` (identity-order queue, largest strictly positive gain
  `2m · w_iC − k_i · K_C` minus the stay quantity, ties to the lowest smallest member),
  `refine` (best choice over singletons in identity order, into a well-connected
  subcommunity) and aggregation (nodes numbered by smallest member), until an iteration moves
  and merges nothing, with no cap. `export` loads the rows as `Export::communities`.
- `lang/mod.rs`: the **language adapter**, `LanguagePack` (extensions, `parse` to `Facts`,
  `config_files`, `modules`, `receiver`: a keyword naming the enclosing class's
  instance, `this` for TS/JS, none for Python, `base_receiver`: a keyword naming its base, whose
  members are not resolved, `super` for TS/JS, none for Python, whose `super()` is a call, and
  `globals`: names bound everywhere that contract kinds read through `Code::binds_everywhere`,
  `fetch` for TS/JS, none for Python), and the `Facts`/`Value` types, plus the tree-sitter
  helpers (`named`, `children`, `text`) every pack shares. `PACKS` lists `lang/ts.rs` (`.ts .tsx
  .js .jsx .mjs .cjs`, reads `tsconfig.json`), `lang/python.rs` (`.py`, reads
  `pyproject.toml`: `.pyi` and notebooks are not read), `lang/rust.rs` (`.rs`, reads every
  `Cargo.toml` through the `**/Cargo.toml` config name, receiver keyword `Self`, see below) and
  `lang/go.rs` (`.go`, reads every `go.mod` through `**/go.mod`, see below), then the Java,
  Kotlin and `lang/csharp.rs` packs (`.cs`, reads every project file through `**/*.csproj`
  with `dotnet::msbuild`).
  `refresh` reads a `**/<name>` config at any depth from the walk, in path order (and a
  `**/*.ext` one, every file ending so, `lang::names_config`), and any
  config through `read_config` (up to one byte past `lang::MAX_BYTES`, no symbolic link,
  as a manifest is); it joins each pack's files first (`LanguagePack::join`, built from the
  configs and every file's facts as parsed, applied to each file and to each file the
  contracts parse again: C#'s project-wide usings and `Facts::unit`), then groups the
  parsed files by pack, asks each pack once for its `layout(configs, parsed files)`, keeps the
  files `Layout::members` names (default: all; a left-out file keeps its `files` row and facts
  with an empty `resolved`, its symbol and call rows are deleted, and the export and `status`
  skip it), and builds each `Resolver` from `Layout::modules`, `Layout::published` and
  `Layout::reaches`; the `Resolver` reads a type declared in parts in several files of one unit
  (`Facts::partial`, `Facts::unit`) as one type (`Resolver::parts`). `parse` sees one file at a time; no pack
  method touches the index. `Facts::locals` holds every name a binding makes besides definitions, `const`s and
  `Def::params` (`let`, `var`, destructuring, `catch`, `for ... of`, a nested, array or rest
  parameter pattern, any parameter of a function that is no definition), qualified by the
  enclosing definition; `Resolver::lexical` checks it first at each scope, so it hides an outer
  name and a definition, constant or parameter of its own scope alike (`Binding::Hidden`: a
  call falls to a name match, a value is not known), the hidden names probe. `Call::uses` (spec #23, `serde(skip)`) is the pack's optional shape
  facts: each chain of `Step`s (`Field`, `Element`, `Call(method)`, `Typed(Ty)`, `Argument(tys)`,
  the type arguments a call is given, `Return`) the function
  making the call applies to its result; the TS/JS pack fills it in `lang/ts/uses.rs` (through
  `await`, parentheses, casts, member and literal element accesses, method calls, and the uses
  of the names a `const` binds, a destructuring pattern (`PATTERN_DEPTH` deep; each field or
  element it names is a read), the variable of a `for...of` and the first parameter of an
  array method's callback written in place (`ELEMENT_METHODS`), each only when nothing else in
  the function binds or assigns the name; `Return` only from the call's own function), within
  `USES_BUDGET` per file. `Def::returns` holds a function's return type annotation as a `Ty` (`serde(skip)`, reparsed). Types (spec #23): `Ty` (`Object`, `Named`, `Intersection`, `Array`, `Schema`, `Param`, a type parameter of the
  enclosing function, `Unknown`) is what `lang/ts/types.rs` reads from a type; an `interface` or `type` declaration is a `Const` holding `Value::Type(Ty)` (so lexical, import and export resolution find it; `Code::held` skips it and `Code::declared_type` finds it), `Const::ty` and `Call::arg_types` hold the type a constant or argument is annotated or cast to (`serde(skip)`), and `declared.rs` (`Types`) expands a `Ty` into its field paths, optional marks and open (index signature) paths, with `TYPE_DEPTH` and `TYPES_BUDGET`. `Value` has `New(callee, args)` (`new X(...)` as a value), `List` (an array literal),
  `Call(Invocation)` (the callee, the receiver value of a method call and its arguments, so a
  chain such as `z.object({..}).optional()` is read, and the call's range in `Facts::calls`,
  which `Code::origins` resolves to follow what it returns), `Member(object, property)` (`schema.shape`),
  `Schema(name)` (a constant annotated `z.infer<typeof name>`, from `lang/ts/types.rs`) and
  `Scalar(Option<u32>)` (a number or boolean literal, spec #23; a plain integer carries its value,
  a status), and `Code::is_param` and `Code::is_global` tell a name that is the handler's own
  parameter, or a global such as `Response`, from one a local binding hides;
  `property(object, key)` (the last key, through spreads; `Err(Depends)` when a spread of a
  parameter may set it) and `literal(parts)` are shared by kinds and packs. An anonymous function
  written as a property of an options object passed as an argument (`run({ eachMessage: () =>
  {} })`, or an object method there) is a `Kind::Inline` definition and a `Value::Function`,
  like a function argument. The channel kind's readers (`channel/{ts,python,rust,go}.rs`, each
  `messages(code) -> Vec<Message>`, answering for its own pack; the types are in
  `channel/message.rs`) give the **producer and consumer output** (spec #20): `Message {broker, name: Option<String>, path, call, role}`,
  `role` (`Side`) `Producer` or `Consumer(Handler)`, `Handler::Def {path, name}` (a definition; a name
  that folds to nothing is the top level, so a pack passes a call's `scope` for "the enclosing
  function") or `Handler::Inline {path, def}`; `Side::Publish {exchange}` (a publish to a named
  exchange, `name` the routing key) and `Side::Binding {exchange, pattern}` (`name` the queue)
  are the RabbitMQ roles that are neither a plain producer nor a consumer. `name` is `None`
  when it cannot be evaluated (an unresolved side). A reader asks `Code` (values, `Code::origins`,
  `Code::outside`, `Code::expand`) and reports; the pack holds none of it, and the channel kind turns the output into contracts. TS/JS library
  forms live in `channel/ts.rs`: kafkajs `send({topic})` on a `new Kafka().producer()`,
  `subscribe({topic})` or `({topics})` on a `new Kafka().consumer()`, with the handler of the
  first `run({eachMessage|eachBatch})` on the same consumer binding (else the function whose
  call subscribes); AWS SDK v3 `SendMessageCommand` / `ReceiveMessageCommand` (the queue is the
  last segment of the `QueueUrl`, so only that tail has to evaluate; the receive's handler is
  its enclosing function); `sqs-consumer` `Consumer.create({queueUrl, handleMessage})`; NATS
  `publish(subject)` / `subscribe(subject, {callback})` on a `connect()` connection (the
  handler is the `callback`, else the enclosing function); BullMQ `add` on a binding that holds
  `new Queue(name)` (the queue is named by the binding, read with `Code::expand` over `Expand::Constructed`:
  the `add` call is the producer, or, when `name` is a parameter of the function that makes the
  binding, each call site of that wrapper) and `new Worker(name,
  processor)` (the processor, a definition or a function in place, is the handler); ioredis
  `xadd(stream, ...)` and `xreadgroup(..., 'STREAMS', s1, s2, id1, id2)` (every stream of the
  first half after `STREAMS`; the enclosing function handles) on a `new Redis()` or default
  import; node-redis `xAdd(stream, ...)` and `xReadGroup(group, consumer, {key} | [{key}])` on a
  `createClient()` binding; and amqplib on a channel that
  traces to `connect().createChannel()` or `createConfirmChannel()` (default import, namespace
  or named `connect`): `sendToQueue(queue)`, `consume(queue, handler)` (a function in place or
  a definition; else the enclosing function), `publish(exchange, key)` and `bindQueue(queue,
  exchange, pattern)`, each name read through `Code::expand` like a topic. `new X(...)` is a
  call fact like `X(...)` (the `new_expression` arm of `ts.rs`'s `calls`, callee `X`), so a
  form can start at a constructor, and `value` reads through `await`, so `const nc = await
  connect()` or `const channel = await conn.createChannel()` is a traced binding. Both are in
  every export (`Kafka#0` and the like: unresolved calls, no target).
