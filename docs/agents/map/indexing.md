# Discovery, index and resolution

Detail behind the [architecture map](../architecture.md), which keeps the flow, identities,
seams, invariants and Known ceilings. The code wins over this file, and a change to these
modules updates it.

- `workspace.rs`, `config.rs`: `Workspace::discover` finds `Repository { root, identity, config }`
  from the `meada.toml` files (peers by `path` hint, then sibling clones; siblings that name a
  repository already in join, transitively; a sibling with an invalid `meada.toml` or
  `meada.decisions.toml` is skipped). Peers not found go to `Workspace::missing`. `config` parses
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
- `repo.rs`: root, identity (normalized `origin`, else `local/<dir>`) and `uncommitted()`, all by
  shelling out to `git`.
- `entry.rs`: entry points (spec #31): `find(&Local)` reads what `refresh` has at its end (the
  symbols, the targets of every call, the `exports` rows, the providers the code gives, the
  packs' `program_entries`) and
  returns the repository's `EntryPoint`s (identity, `EntryKind`, label, `site`), which
  `refresh` rewrites into `entry_points` and `export` loads into `Export::entry_points`.
- `index.rs`: SQLite schema, `refresh`, `export` and the export types. Freshness is a pipeline in
  `refresh`: stat (mtime/size, trusted only if older than `RACY` = 2 s), hash with BLAKE3, parse
  only changed files, resolve every file's calls in memory, rewrite rows only when a file's
  `resolved` digest changed, re-extract contracts for the whole repository (one row per side, the
  `Found::shape`s of a side reached several ways merged, as JSON in `contracts.shape`, which `export` loads into `Provider::shape` and
  `Consumer::shape`). `refresh(db, root,
  identity, serve)` first brings the `documents` table (path, hash, mtime, size, `summary`) up
  to date the same way (`refresh_documents`, sharing `stat`), before the early exit and apart
  from the code rows: a document edit re-extracts no code. Without `serve.declared` the
  repository has no documents and stored rows are deleted. The walk yields every file the
  ignore rules keep; `lang::pack_for` and `document::is_document` split it, and
  `documents_found` adds each `[serve] documents` path that is a file, stat'ed outside the walk
  so `.gitignore` does not hide it; a listed path that is not a file, or that has a symbolic
  link in any segment (`plain_file`; the walk follows none either), has no row. `export` loads
  `Export::documents` (`Document {path, summary}`, path order). `refresh_stamped(tx, table, ...)`
  is the shared pass behind `documents` and `manifests` (spec #22): the `manifests` table (path,
  hash, mtime, size, `summary` as `package::Summary` JSON) holds every `package.json` the walk
  finds (`manifests_found`: any path with a `node_modules` segment is skipped, since the walk
  yields it unless `.gitignore` lists it), a nameless one too, whatever `serve.declared` says;
  `export` loads them as `Export::manifests` (`serde(skip)`), and the `tsconfig.json` rows
  (`manifests_found` takes them too, `package::read_tsconfig`) as `Export::tsconfigs`. With the contracts, `refresh`
  rewrites the `exports` table (`Resolver::exported` of every pack: per file, each exported
  name with the `file` and `target` definition, constant or type it names, `target` NULL for a whole file and
  `file` NULL for nothing the repository holds, and each `export * from` a repository file as a
  row with `name` NULL), which `export` loads as `Export::exported`, and the `import` rows of
  `contracts`, loaded as `Export::import_calls` (both `serde(skip)`). Before extracting,
  `refresh` computes the packages' public surface (`package::surface`, from the `manifests` rows
  read by `read_manifests`, the `exports` rows of every pack and the files), and the extraction
  loop runs `package::exported_shapes` over each pack's `Code` for its files, whose `shape` rows
  `export` loads as `Export::shapes` (`serde(skip)`, issue #175). Every `manifests` row is folded
  into the configs digest, so an edited `package.json` or `tsconfig.json` extracts again. `sort_providers` and
  `sort_consumers` put the lists back in `export`'s order after a kind's `declare` added sides.
- `community.rs`: communities (spec #31). Last in `refresh`, after the code rows,
  `write_communities` rewrites the `communities` table (name, member) from the `calls` rows at
  `exact` or `inferred` whose caller and target are both symbols (a top-level caller is a
  file, no node), so an unchanged repository keeps its rows through the early exit and a
  changed one recomputes them from scratch. `communities(calls, dir)` builds the undirected
  graph (a call site adds 1 to its edge, a call to self none) and runs Leiden as spec #31 fixes
  it, in `i128` only: `local_moving` (identity-order queue, largest strictly positive gain
  `2m · w_iC − k_i · K_C` minus the stay quantity, ties to the lowest smallest member),
  `refine` (best choice over singletons in identity order, into a well-connected
  subcommunity) and aggregation (nodes numbered by smallest member), until an iteration moves
  and merges nothing, with no cap. `export` loads the rows as `Export::communities`.
- `lang/mod.rs`: the **language adapter**, `LanguagePack` (extensions, `parse` to `Facts`,
  `config_files`, `modules`, `messages`, `program_entries`), and the `Facts`/`Value` types. `lang/ts.rs` is the only
  pack (`.ts .tsx .js .jsx .mjs .cjs`). `parse` sees one file at a time; no pack method touches
  the index. `Facts::locals` holds every name a binding makes besides definitions, `const`s and
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
  chain such as `z.object({..}).optional()` is read), `Member(object, property)` (`schema.shape`),
  `Schema(name)` (a constant annotated `z.infer<typeof name>`, from `lang/ts/types.rs`) and
  `Scalar(Option<u32>)` (a number or boolean literal, spec #23; a plain integer carries its value,
  a status), and `Code::is_param` and `Code::is_global` tell a name that is the handler's own
  parameter, or a global such as `Response`, from one a local binding hides;
  `property(object, key)` (the last key, through spreads; `Err(Depends)` when a spread of a
  parameter may set it) and `literal(parts)` are shared by kinds and packs. An anonymous function
  written as a property of an options object passed as an argument (`run({ eachMessage: () =>
  {} })`, or an object method there) is a `Kind::Inline` definition and a `Value::Function`,
  like a function argument. `messages(code) -> Vec<Message>` (default: none) is the **producer
  and consumer output** (spec #20): `Message {broker, name: Option<String>, path, call, role}`,
  `role` `Producer` or `Consumer(Handler)`, `Handler::Def {path, name}` (a definition; a name
  that folds to nothing is the top level, so a pack passes a call's `scope` for "the enclosing
  function") or `Handler::Inline {path, def}`; `Role::Publish {exchange}` (a publish to a named
  exchange, `name` the routing key) and `Role::Binding {exchange, pattern}` (`name` the queue)
  are the RabbitMQ roles that are neither a plain producer nor a consumer. `name` is `None`
  when it cannot be evaluated (an unresolved side). The pack reads facts through `Code` (values, `Code::origin`,
  `Code::expand`) and reports; the channel kind turns the output into contracts. TS/JS library
  forms live in `lang/ts/messages.rs`: kafkajs `send({topic})` on a `new Kafka().producer()`,
  `subscribe({topic})` or `({topics})` on a `new Kafka().consumer()`, with the handler of the
  first `run({eachMessage|eachBatch})` on the same consumer binding (else the function whose
  call subscribes); AWS SDK v3 `SendMessageCommand` / `ReceiveMessageCommand` (the queue is the
  last segment of the `QueueUrl`, so only that tail has to evaluate; the receive's handler is
  its enclosing function); `sqs-consumer` `Consumer.create({queueUrl, handleMessage})`; NATS
  `publish(subject)` / `subscribe(subject, {callback})` on a `connect()` connection (the
  handler is the `callback`, else the enclosing function); BullMQ `add` on a binding that holds
  `new Queue(name)` (the queue is named by the binding, read with `Code::expand_constructed`:
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
- `resolve.rs`: the shared resolution behind the adapter. `Resolver` connects calls to definitions
  (scope, imports, re-exports, `this.method`, member chains) and gives each a `Resolution`. `Code`
  folds constants and client wrapper arguments into values for contract kinds, a destructured
  object parameter reading its names (`Param::fields`) off the literal object the caller passes.
  `Code::expand_frames` is `expand` that also returns, for each site, the wrapper calls it
  climbed (`Frame {def, path, index}`, innermost first), and `Code::uses(path, index)` reads
  `Call::uses` like `args` (a file without full facts is a miss).
  `Code::expand` climbs only callers that resolved exact: a call found only by a name match
  (`Caller::by_name`: `data.get(...)` matched to a wrapper method, or a bare name no import
  binds) may be on any object or reach another function, so a wrapper with no exact caller is
  read where it stands.
  `Code::origin(path, scope, chain) -> Option<Origin {package, expr, binding}>` is the
  **receiver tracing** every message library form relies on: it follows a name or member chain
  through `const` bindings (same file and imported) down to an import whose module is outside
  the repository (`Binding::External` keeps the specifier and imported name), and spells what
  the code did with it from that name: `new Kafka().producer()` for `kafkajs`. A namespace import
  adds no name, a default import is `default`; `binding` is the constant the chain's head names
  (how a pack pairs calls on the same client). `Code::expand_constructed(binding, at, read)` is
  `expand` over the arguments of the `new` a binding holds, on behalf of the call `at` made on
  it. `Code::pack()` is the pack of the files. `Code::imported(path, scope, chain)` is the same
  tracing without `const`s: the specifier and spelled chain when the chain's head is itself an
  import from outside the repository (what `Packages::extract` stores). `Resolver::exported()` is
  what each file exports, as `export` reads it, for the `exports` table.
