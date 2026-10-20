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
  symbols, the targets of every call, the `exports` rows, the providers and consumers the code gives, the
  `Facts::programs` the packs set, and the console scripts of the `manifests` rows through
  `package::python_programs`, and `Layout::programs`, Rust's binary roots) and
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
  `config_files`, `modules`, `messages`, `receiver`: a keyword naming the enclosing class's
  instance, `this` for TS/JS, none for Python, `base_receiver`: a keyword naming its base, whose
  members are not resolved, `super` for TS/JS, none for Python, whose `super()` is a call, and
  `globals`: names bound everywhere that contract kinds read through `Code::binds_everywhere`,
  `fetch` for TS/JS, none for Python), and the `Facts`/`Value` types, plus the tree-sitter
  helpers (`named`, `children`, `text`) every pack shares. `PACKS` lists `lang/ts.rs` (`.ts .tsx
  .js .jsx .mjs .cjs`, reads `tsconfig.json`), `lang/python.rs` (`.py`, reads
  `pyproject.toml`: `.pyi` and notebooks are not read), `lang/rust.rs` (`.rs`, reads every
  `Cargo.toml` through the `**/Cargo.toml` config name, receiver keyword `Self`, see below) and
  `lang/go.rs` (`.go`, reads every `go.mod` through `**/go.mod`, see below).
  `refresh` reads a `**/<name>` config at any depth from the walk, in path order, and any
  config through `read_config` (up to one byte past `package::MAX_BYTES`, no symbolic link,
  as a manifest is); it groups the
  parsed files by pack, asks each pack once for its `layout(configs, parsed files)`, keeps the
  files `Layout::members` names (default: all; a left-out file keeps its `files` row and facts
  with an empty `resolved`, its symbol and call rows are deleted, and the export and `status`
  skip it), and builds each `Resolver` from `Layout::modules`, `Layout::published` and
  `Layout::reaches`. `parse` sees one file at a time; no pack
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
  like a function argument. `messages(code) -> Vec<Message>` (default: none) is the **producer
  and consumer output** (spec #20): `Message {broker, name: Option<String>, path, call, role}`,
  `role` `Producer` or `Consumer(Handler)`, `Handler::Def {path, name}` (a definition; a name
  that folds to nothing is the top level, so a pack passes a call's `scope` for "the enclosing
  function") or `Handler::Inline {path, def}`; `Role::Publish {exchange}` (a publish to a named
  exchange, `name` the routing key) and `Role::Binding {exchange, pattern}` (`name` the queue)
  are the RabbitMQ roles that are neither a plain producer nor a consumer. `name` is `None`
  when it cannot be evaluated (an unresolved side). The pack reads facts through `Code` (values, `Code::origins`,
  `Code::outside`, `Code::expand`) and reports; the channel kind turns the output into contracts. TS/JS library
  forms live in `lang/ts/messages.rs`: kafkajs `send({topic})` on a `new Kafka().producer()`,
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
- Python message forms live in `lang/python/messages.rs`, which finds the client of a call with
  `Code::origins` with `Follow::Values` (which follows `self.x`, parameters and returns, as
  `Follow::Constants` does not) and
  names the constructor it reached from the file's imports (`Code::imported`:
  `confluent_kafka.Producer`, however it is imported; `Code::outside` also answers for a method
  of a typed local, so it is not used). One `channel()` or `client()` above a constructor is
  followed (`pika.BlockingConnection.channel`, `boto3.Session.client`), also when the client is a
  call written in place (`boto3.client("sqs").send_message(...)`): `head` takes the call that
  starts where the outer one does and ends last inside it, since the callee of such a call is
  `(call).method`. A `KafkaConsumer` is told by its resolved name, so an alias counts. Forms: confluent-kafka
  `Producer.produce(topic)` and `Consumer.subscribe(topics)`; kafka-python `KafkaProducer.send(topic)`,
  the topics a `KafkaConsumer(...)` constructor takes and `subscribe(topics=)` (a `^` topic is a regular expression, so it is unresolved); pika
  `basic_publish(exchange, routing_key)` (the empty exchange is a producer of the key),
  `basic_consume(queue, on_message_callback)` and `queue_bind(queue, exchange, routing_key)`;
  boto3 `send_message`, `send_message_batch` and `receive_message` with `QueueUrl=` on a client
  made for `"sqs"`; nats-py `publish(subject)` and `subscribe(subject, cb=)` on `nats.connect()`
  or `nats.aio.client.Client()`; redis-py `xadd(name)` and the keys of `xreadgroup(...,
  streams={...})` on `Redis` or `StrictRedis` (`redis` or `redis.asyncio`, also `from_url`). A polling
  client's handler is the function that subscribes. The pack's `value` reads `await x` as `x`
  (a traced `nc = await nats.connect()`) and a dict literal as `Value::Object` (a key that is
  no string literal is the entry `[]` holding the key and the value, so `Code::expand` folds
  it). `lang::channel_name` and `lang::queue_name` evaluate a channel name and an SQS queue URL
  for both packs.
- `lang/rust/frameworks.rs` (`FRAMEWORKS`): per object of axum, actix-web, Rocket, reqwest and
  ureq, the outside names of its constructors and the methods that hand it back; `RustPack::passes`
  reads it (else `messages::passes`) and the HTTP kind asks it what a constructor makes
  (`frameworks::made`). `VERBS` (the `Method` constants) lives there too.
- Rust message forms live in `lang/rust/messages.rs`, beside the TS and Python ones and without
  a parallel path into `channel.rs`. The `LIBRARIES` table names each crate's constructors
  (suffix-matched on `Code::outside`, without the path flag, over the constructor the
  `Follow::Values` origin reached) and the methods that hand back the same object
  (`passes`, which `RustPack::passes` falls through to, so `flow` follows `set`, `create_channel`,
  `connect` and the like). Forms: rdkafka `send` of a `FutureRecord::to(topic)` or
  `BaseRecord::to(topic)` (the record is found from the argument, down its chain) and
  `subscribe(&[topics])`, on a client from `ClientConfig::new()`; lapin `basic_publish`,
  `basic_consume` and `queue_bind` on a channel of `Connection::connect`; aws-sdk-sqs
  `queue_url` of a `send_message`, `send_message_batch` or `receive_message` builder (the
  queue is the last URL segment); async-nats `publish`, `subscribe`, `queue_subscribe`; redis
  `xadd` and the keys of `xread_options`, only with a `group` option. A polling or stream
  client's handler is the function that holds the call. The shared evaluation (`produce`,
  `consume`, `topic`, `topics`, `publication`, `queue_name`) lives in `lang/mod.rs`, used by
  the three packs. A `let mut` bound once keeps its call as `Facts::mutable`, read by
  `Code::mutable`, so a redis connection held by one is traced.
- `lang/python.rs` yields `Kind::Method` for a function directly in a class body, keeps the first of
  two definitions of one name, spans a decorated definition with its decorators, and exports
  every module-level name (imports included), as an explicit import or `m.x` sees them, and
  its star imports, last binding first, since a later one rebinds the name (`stars_bind`: the
  resolver takes the first export that gives a name, so a star import after a definition
  rebinds it when its module exports it); what `import *` takes is `Facts::star`: the public
  names, or a literal `__all__` alone, grown by the literal names a statement of the module
  itself adds (`+=`, `.extend`, `.append`: `statement`), else `Star::Computed` (also for an
  item set or deleted, `Module::items`, and any other binding of `__all__`). The last binding of a name in its scope
  decides: a definition another binding follows (at the top level, `Module::hidden`; in a
  function, where a nested function's `nonlocal` binding follows any, `Locals::finish`), one
  not `in_place` (under `if`, `try`, a loop, `with`, `match`) after another binding of its
  name, and a name `del` unbinds (in any scope), are `Facts::locals`, which hide the name. A class body's
  binding hides no method, which `Resolver::member` reads through its class.
  Its imports are the module as written (`.`, `..pkg`, `a.b`): `import a.b.c` binds `a`, `a.b`
  and `a.b.c` (dotted `Import::local`s); `from p import m` is an `Imported::Member`, a name
  `p` exports or else the submodule `p.m`; `from p import *` is `Imported::All`. `modules` resolves them against the repository root, then the roots
  `pyproject.toml` declares (setuptools `package-dir` and `packages.find.where`, Poetry
  `packages.from`, read by `lang/python/pyproject.rs` through the PyPI manifest reader, so an
  unreadable file declares nothing), or `src` when none is declared (`pyproject::source_roots`,
  which `package::python_roots` shares); a package comes before a module of the same name.
  An `if __name__ == "__main__":` block at the top level (either operand order, either quote,
  nothing else) yields the definition `__main__`, spanning the `if` and its body (an `else` is
  not part of it), owns the calls in that body, and is named in `Facts::programs`.
  It also yields the facts receivers resolve through: `Def::bases` (a class's bases as
  written, `B[T]` read as `B`, any other expression `(<kind>)`) and `Facts::receivers`, one
  `Receiver {scope, name, class}` per name a scope binds: a method's first parameter unless
  `@staticmethod` (`Class::Enclosing`), an annotated parameter or name, `x = C(...)`
  (`Class::Named("C")`, whatever `C` turns out to be), `self.x: T` in a method (an attribute,
  at the class's scope), and `Class::Unknown`, which hides an outer binding, for any other
  binding: other assignments, `for` and comprehension targets, `with` and `except ... as`,
  `:=`, `case` captures, a lambda's parameters (in the enclosing scope, which owns its calls)
  and a parameter the body assigns. A name its scope binds only once, by `name = value`, is
  also a `Const` holding that value; so is the name of `with C(...) as x` (also
  `Class::Named("C")`, as `x = C(...)` is) and a `self.x = C(...)` attribute (a `Const` of the
  class, only for a call). `xs += ...` and `xs = xs + ...` on a name whose latest
  value in that scope is a list literal do not bind it again: a list literal added joins the
  `Const`'s items, anything else added (`+= static(...)`) is left out. Calls carry their positional `args` and keyword `kwargs`
  as `Value`s (strings, f-strings and `+` folded into parts, a name or a member chain in them,
  `config.BASE`, a `Part::Ref`; `[...]`/`(...)` a `Value::List`, a call
  a `Value::Call` with its callee and range, no receiver or arguments), a decorator call names the definition it decorates
  (`Call::decorates`), a decorator written as a bare name is in `Def::decorators` (as a
  callee, `http.require_safe`), and a function lists the values it returns (`Def::return_values`).
  A function's parameters carry their defaults as written (`Param::default`) and which
  argument binds them (`Param::binds`, a `Binds`): `*args` and `**kwargs` are `Rest` (no single
  argument), the parameters after them or after the bare `*` (which is no parameter) are
  `Keyword`, a method's first one is `Instance` (`Class` for a `@classmethod`, neither for a
  `@staticmethod`), any other `Any`. A target that is `self.x`, also inside unpacking, a `for`
  or an `as`, and also written in a function nested in the method, binds the class's attribute
  `x`. An import in a function or a class is `Class::Imported`: it hides an outer binding there
  and resolves through the file's imports. An annotation decides; other bindings that
  disagree make the class unknown. `global x` sends the function's bindings of `x` to the
  module, `nonlocal x` to the nearest enclosing function that binds `x` (class bodies
  skipped). Annotations are read as written: a name, a string holding one, `Optional[...]`
  and `... | None`.
- `lang/rust.rs` also yields `Facts::macro_use` (the crates of `#[macro_use] extern crate x;`,
  stored) and `Facts::mentions` (`lang/rust/mentions.rs`: the names a file refers to, apart
  from those a pattern, a parameter, a field or an item declares, a reference a local hides and
  its route attributes; not stored, so the providers get it from the files parsed again).
  A function does not hide the crate a path begins with (`rocket::build()` beside a
  `fn rocket`), and a module of the same name as a function still hides the crate.
- `lang/rust.rs` (spec #30) yields free functions (`Kind::Function`), structs, enums, unions
  and traits (`Kind::Class`), modules written in place (`Kind::Module`), methods of an `impl`
  (`Type.method`, or `Type.Trait.method` in a trait's block, `Kind::Method`) and trait methods
  and required methods (`Trait.method`); closures are no definitions, their calls belong to the function
  around them and their parameters are its locals, as `let`, `for`, `if let`/`while let` and
  match arm patterns are. A method's `self` is a `Param` with `Binds::Instance` and a
  `Receiver` of `Class::Enclosing`. Receivers (`Facts::receivers`) also come from a parameter
  or a `let` of a plain name with a written type, from a struct's named field (at the struct's
  scope), and from a `let` holding a call of a name or a path (`Class::Returned`, the callee as
  the call spells it: the resolver gives the class `Def::result` names, `Self` included, the
  class itself for a constructor, and an outside crate's call stays outside). A type is read
  as written (`written`): `&T` and `&mut T` as `T`, `dyn Trait`, `impl Trait` and a type
  parameter (`<T: Trait>` or `where T: Trait`, of the function, `impl` or struct around)
  bounded by exactly one trait as that trait, a primitive or a path as a path normalized like a
  call's (an outside crate's or the prelude's type, `Box<T>` and `Vec<T>` included, is in `Facts::externs`, so opaque); anything else (tuples,
  slices, several bounds, `T::Item`) names no type, and a `let` of it is a local
  that hides the name. A `let` with a written type and one that disagrees, in a scope, leave
  the class unknown. `Def::result` is the same reading of a function's return type. Each `impl`
  is a `Facts::impls` entry (its type and trait as callee-like names, from the scope around
  it); a block whose method names one already defined takes the next free `#n` scope
  (`E.From#2`), so each body is its own symbol. A `mod x;` declaration is a `Kind::Module`
  definition `x` (`inner.x` in a module written in place, the declaration's span) and an
  import of the whole file under that name, its specifier `mod x` / `mod inner/x` or, with
  `#[path = "p"]`, `path |p` / `path inner|p`; at the top level the file also exports it.
  Every top-level item and `use` name is exported (`Facts::exports`, `Facts::private` for the
  ones not `pub`, which the exports table leaves out); the `pub` ones are `Facts::star`, and a
  `pub` item of a `pub` module written in place is also exported under its dotted name. Calls: an identifier, a
  member chain (`Call::by_path` false) or a path (`by_path` true, dotted by `normalize` once
  the file is read: `crate.…` and `super.…` with their synthetic imports `crate` and
  `super(.super)`, `Self.…`, `self::` and in-file `super::` as `mod.<module>.…` with the
  synthetic import `mod` of the file itself (`Tree::lookup("self")`), and a first segment no
  item of the module or `use` of the scope binds a crate outside the file, `Facts::externs`).
  A `use` leaf is an `Imported::Path` import (`use a::{b, c as d, e::{self}}` binds `b`,
  `d`, `e`; `local` is qualified by the scope holding the `use`; `as _` and `use serde;` bind
  nothing) or an `Imported::Glob` import (`local` is `*` qualified the same way), its path
  normalized like a call's, `use super::*` and `use self::*` being `inside`. Macro invocations
  are not read. Conditional compilation (`lang/rust/cfg.rs`): `parse` skips a node whose
  preceding `#[cfg(..)]` attributes are not all true (`compiled`), for the fixed target
  `x86_64` Linux (`unix`, `target_os = "linux"`, `target_family = "unix"`,
  `target_arch = "x86_64"`, `target_pointer_width = "64"`, `target_endian = "little"`,
  `target_env = "gnu"`, `target_vendor = "unknown"`, `panic = "unwind"`, `debug_assertions`,
  `target_has_atomic` (8 to 64, `ptr`), `target_feature` (`sse`, `sse2`, `fxsr`), the literal
  `true`; `test` and any other predicate are false) and the crate's default features:
  `[features] default` followed through features that enable others (`dep:x` entries ignored;
  `x/feat` enables the feature `x` of an optional dependency that no `dep:x` names, `x?/feat`
  does not). `test` is false, as in a build of the crate: `cfg(not(test))` items are indexed.
  The exception is a predicate that is exactly `test`: a `mod` under it is indexed, any other
  item under it is not (a `#[cfg(test)] use` would shadow the production one).
  `#[cfg_attr]` is no skip; a cfg-false `mod x;` makes no import, so its file is no member.
  `built` decides the children of a node in one pass over the list, carrying the pending
  attributes (and the `#[path]` value) forward. An inner `#![cfg(..)]` that is false drops the
  module written in place, or the whole file (`Facts::unbuilt`, which keeps it out of the
  module tree). The features reach `parse` as `Facts::setting` (`LanguagePack::settings` gives
  it per path, `parse_with` takes it; the index keeps it in `files.setting`): `refresh` parses a
  file again when the setting it was parsed under differs, so a toggled default feature gives
  the cold export. The decider's `function_like` reads the nearest `Cargo.toml` files from disk
  for the same setting. `lang/rust/crates.rs` (`Tree`) is the module tree behind the
  Rust `layout`, built once per refresh (each `Cargo.toml` parsed once for it; `settings` parses
  them once more, before any file is): the crate roots of every `Cargo.toml` with a `[package]`, at any
  depth (`[lib] path`, else `src/lib.rs`, `src/main.rs`, `[[bin]] path`, `src/bin/*.rs`,
  `src/bin/*/main.rs`), walked breadth first
  through each file's `mod` imports (`x.rs` or `x/mod.rs` under the directory the file owns:
  its own for a root, a `mod.rs` or a `#[path]` file, the one named after it otherwise; a
  `#[path]` from the file's directory, or from the inline module's directory inside one).
  `lookup` answers `crate` (the root), `self` (the file), `super` (`super::super`, through the top-level
  declarations that reached the file), a `mod` specifier (its file) and a crate's name (the
  root of its library, `[lib] name` else the package name, hyphens as underscores; a binary is
  no library), reached only from the crates whose manifests list it as a dependency
  (`Tree::libs`, per crate root; `dependencies()` reads the three tables, a `package =` rename and
  `workspace = true`). `Tree::reaches` (`Layout::reaches`) answers whether a name match from
  one file may land in another: one crate builds both, or the second is in a library of
  `Tree::reach` for the first's crate root (its `libs`, then each one's `[dependencies]`, all
  the way down: a dev or build dependency of a dependency is not part of it).
  `Tree::published` (`Layout::published`) walks from each library root through `pub mod`
  declarations (`Export::From` whose source is the specifier, not in `Facts::private`): the exports table keeps the `pub` items of those files alone, and a binary
  root exports nothing (no other crate can import it), so no `export` entry point either.
  `lang/rust/values.rs` (`Reader`) reads values for contract kinds (axum, spec #30 #129): each
  call's `Call::args` and `Call::receiver` (the value a builder call is made on,
  `LanguagePack::fluent`), a closure argument as a `Kind::Inline` def named `@<byte offset>` and a
  `Value::Function`, a `const`, `static` and a `let` bound once in its scope as a `Const`, and a
  function's `return_values` (its tail expression and `return`s); `Put` queues each until the
  file's `use`s are known, so a path normalizes like a call's. Depth is bounded by `VALUE_DEPTH`.
  Strings fold (spec #30 #131): `+`, `format!`/`concat!`, `String::from` and the conversions
  that keep the text, `Method::GET` of the HTTP crates (`Scan::targets` holds what each `use`
  imports), and a `let mut` String that `lang/rust/builders.rs` finds a plain builder
  (`parse` collects them per scope and name; the Reader substitutes the text the pushes before
  the read position hold, memoized per push).
  `args`, `receiver` and `return_values` are not stored (the constants arrived with schema version 28).
- `lang/go.rs` (spec #25) yields functions (`Kind::Function`), methods named by their
  receiver's type (`Type.Method`, `*T` and `T[K]` alike, `Kind::Method`), every named type
  and alias (`Kind::Class`) and interface methods (`Interface.Method`). A function literal is
  a `Kind::Inline` definition (`@<offset>`, which the index names or folds into the one around
  it, so its calls belong to the enclosing symbol), its parameters bound in it; so is a blank
  `func _()`, which folds into the file. A second `init` of a file is `init#2` (and so on).
  Top-level `const` and `var` names are `Facts::consts` (no value yet); names bound inside a
  function (`:=`, `var`, `const` and `type` in a block, `range`, `select` receives, type switch
  bindings, named results, type parameters, a method's receiver and the type parameters its
  type arguments name) are its locals, unless every binding of the name in the function writes
  one class (`Walk::finish`): then it is a `Receiver` (`Class::Type` for a parameter, the method
  receiver, a named result, a `var` with a type, a composite literal; `Class::Called` for a
  call, whose function's `Def::result` is its first result type). A struct's named fields are
  receivers in the struct's scope. Exports are the top-level names that start with an
  upper-case letter. Imports: plain (`assumed_name`) and aliased ones are
  `Imported::Namespace`, a dot import `Imported::All`, a blank one nothing; a name two imports
  give is imported from the empty specifier, which names nothing (`unbind_clashes`). `parse`
  evaluates the build constraints (`lang/go/build.rs`: file name suffixes and the
  `//go:build` line of the header, read as the Go tools read it: before the first line of code,
  outside block comments) for `linux/amd64` and marks a file they exclude `Facts::unbuilt`,
  which `layout` leaves out of `members`. `layout` reads the `go.mod` files once
  (`lang/go/modfile.rs`: `module` and `replace` directives, a line format read with no parser
  crate) and answers `modules` with the first non-test file of the directory an import names,
  which stands for the package (`Modules::dir`: a `replace` of the importing file's module,
  none when it is by a module from outside, else the longest module path of the repository, a
  directory its module owns), and `shared` with the files of each directory and
  `Facts::package`.
- `resolve.rs`: the shared resolution behind the adapter. `Resolver` connects calls to definitions
  (scope, imports, re-exports, `this.method`, member chains) and gives each a `Resolution`. A name may be
  bound by several imports (the latest in source order decides, whatever it resolves to), a
  callee may start with a dotted import, and an `Imported::All` import counts as one binding
  of each name its module's `Facts::star` takes. `lexical` skips a class's scope unless the
  lookup starts in it (a class body's names are not in scope for its methods). A member of a
  class, or of a parameter or local whose `Class` resolves to a class of the repository
  (`Binding::Instance`), is looked up through the class's family: the class and the ancestors
  its written bases give, in C3 order (Python's method resolution order), linearized once per
  class in `Resolver::new`. A base that is not a class of the repository is an opaque
  ancestor, named by what the file imports for it (`abc.ABC`) or as written (`object`), so
  classes that share it give C3 one entry; a base that is no name (`mixin()`) is its own
  entry: reaching it before a class that has the member leaves the member unknown, and so
  does a hierarchy C3 rejects, bases in a cycle or a family longer than `FAMILY_DEPTH` (cut
  without being kept on the way up when the walk down the bases reaches that depth, `Cut`). The first class that defines the member, or
  binds it as an attribute (a `Receiver` of the class), wins, `exact`. A receiver whose class (or whose value's
  callee) is outside the repository is unresolved; any other member call falls back to the
  name match. `Busy` guards locals and families that resolve through themselves. A single
  module-level binding is read from functions as it is; a function that rebinds it through
  `global` makes it unknown. A pack that writes `Def::bases` and `Facts::receivers` gets this
  without resolver changes (TypeScript writes none yet; its `this` is the pack's `receiver`
  keyword). A class's blocks (`Facts::impls`: Rust `impl`s, in any file, their type and trait
  resolved from `Impl::within` once in `Resolver::new` into `impls` and `blocks`) add members:
  `member` tries `block_member` first (the inherent blocks' `scope.m`, else the trait blocks'
  `scope.m` or their traits' default `Trait.m`; several at that step are `Binding::Several`,
  which `call` answers as `Ambiguous` with exactly them), then the class's own `Class.m`, and
  `class` (the `receiver` keyword, `Self`) answers the block's type inside a block. `instance`
  reads `Class::Returned` through `returned`: the callee resolved as a call (a path with
  `path_binding`), its definition's `Def::result` resolved from the function's own scope;
  `Class::Called` (Go) reads the callee as a value expression instead, so `h.Counter()` follows
  the receiver `h` to its class's method (`returned(.., value)`). A
  `Kind::Module` definition is a member chain like a function (`inner.helper`); one an import
  of its own name binds to a file (`mod store;`) has that file's exports as members; `lexical`
  stops climbing at one, so a module written in place does not see the names around it (only
  the file's imports). `call` reads a `Call::by_path` head with `lexical_as(types)`: no local,
  parameter or receiver hides it. Name inference keys a function or class of modules written
  in place by its last name, like a top-level one, and a call with `Call::by_path` matches
  functions as well as methods by that name; `infer` keeps the candidates the calling file
  reaches (`Resolver::reaching`, from `Layout::reaches`), so a Rust call never matches a crate
  it does not depend on.
  `Code`
  folds constants and client wrapper arguments into values for contract kinds, a destructured
  object parameter reading its names (`Param::fields`) off the literal object the caller passes.
  `Code::expand` returns, for each site, the wrapper calls it
  climbed (`Expanded::frames`, `Frame {def, path, index}`, innermost first), and `Code::uses(path, index)` reads
  `Call::uses` like `args` (a file without full facts is a miss). An argument binds a parameter as its `Param::binds` says: by position or by
  the parameter's name among the keyword arguments, only by name, or not at all (`Rest`); a
  method called on an instance (`x.m(a)`) passes nothing for an `Instance` receiver, one called
  on its class (`C.m(obj, a)`) passes it first, and a `Class` receiver is never passed.
  `Code::expand(Expand::Call(path, index), read)` hands
  `read` the call's positional and keyword arguments, folded, and walks the wrappers' callers
  with them; a call found only by a name match (`data.get(...)` matched to a wrapper method,
  or a bare name no import binds; `Resolver::callers` keeps `exact` calls alone) is not climbed, since it may be on any
  object or reach another function, so a wrapper with no other caller is read where it stands. `Code::origins(path, scope, of, follow)` is
  the one origin question: what made the value of a name or a value as written (the call of a
  library's constructor or factory, or what the source declares a name is), with the methods
  called on the way, and every route the walk took to it. The evidence policy picks the walk, and
  each reader asks the one it needs. `Follow::Values` (Python, Rust, Go's routers and request
  clients, Express) is the shared value flow: the call sites whose result a name may hold,
  followed through constants, imports and module attributes, the arguments every caller passes to
  a parameter (a caller found only by a name match is not climbed, as in `Code::expand`), the
  values a function of the repository returns (its parameters then bound by that call only) and
  the items of a list or tuple; an attribute assigned a call once in its class
  (`self.client = Client()`) counts as a constant; a call that reaches no function of the
  repository is the origin. A constant, parameter or function met again on one path (a cycle) adds
  nothing. `Follow::Constants` (the TypeScript forms) follows `const` bindings alone (same file
  and imported) down to an import whose module is outside the repository, so a parameter or a
  return value has no origin; the answer carries the form of the call that made the value (`new`, a call or the bare
  name), what it names outside, the methods called on the way and, as `held`, the constant the
  name refers to, which pairs the calls made on the same client. `Follow::Declarations` (Go's
  typed clients and forms) reads what the source declares the name is: a written type, or the
  call it was declared from through the methods called on the way. `Code::outside(path, scope,
  name, by_path)` names what an import from outside the repository binds a name to
  (`fastapi.APIRouter`, `express.default`): the package and name there, the name as the file's
  imports spell it and whether its head is the import itself. Contract kinds recognize framework
  constructors with it; with `by_path` the name is a path (Rust's `a::b::f()`) whose head is read
  among types and modules. `Code::expand(Expand::Constructed { held, at }, read)` is the
  expansion over the arguments of the `new` a `held` constant holds, on behalf of the call `at`
  made on it. `Code::pack()` is the pack of the files.
  `Code::definition` resolves a name or a member chain (`views.index`) to a definition,
  `Code::module` a name to the file it binds as a whole module, and `Code::import_file` an
  import specifier (`shop.orders.urls`) to its file.
  `Code::imported(path, scope, chain)` reads the head alone, without `const`s: the specifier and
  spelled chain when the chain's head is itself an import from outside the repository (what `Packages::extract` stores). `Resolver::exported()` is
  what each file exports, as `export` reads it, for the `exports` table.
