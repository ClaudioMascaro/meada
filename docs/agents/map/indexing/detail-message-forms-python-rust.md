# Detail: message forms, Python and Rust

- Python message forms live in `channel/python.rs`, which finds the client of a call with
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
  it). `message::channel_name` and `message::queue_name` (`channel/message.rs`) evaluate a channel name and an SQS queue URL
  for every reader.
- `http/rust/frameworks.rs` (`FRAMEWORKS`): per object of axum, actix-web, Rocket, reqwest and
  ureq, the outside names of its constructors and the methods that hand it back; the HTTP
  kind's readers hand `passes_on` to the question (`Follow::Values { passes }`) and ask what a
  constructor makes (`frameworks::made`). `VERBS` (the `Method` constants, in `lang/rust.rs`) is
  the one pack constant a kind reads: the pack folds `Method::GET` at parse time with it.
- Rust message forms live in `channel/rust.rs`, beside the TS and Python ones and without
  a parallel path into `channel.rs`. The `LIBRARIES` table names each crate's constructors
  (suffix-matched on `Code::outside`, without the path flag, over the constructor the
  `Follow::Values` origin reached) and the methods that hand back the same object
  (`passes`, the predicate the channel reader hands the question, so `flow` follows `set`, `create_channel`,
  `connect` and the like). Forms: rdkafka `send` of a `FutureRecord::to(topic)` or
  `BaseRecord::to(topic)` (the record is found from the argument, down its chain) and
  `subscribe(&[topics])`, on a client from `ClientConfig::new()`; lapin `basic_publish`,
  `basic_consume` and `queue_bind` on a channel of `Connection::connect`; aws-sdk-sqs
  `queue_url` of a `send_message`, `send_message_batch` or `receive_message` builder (the
  queue is the last URL segment); async-nats `publish`, `subscribe`, `queue_subscribe`; redis
  `xadd` and the keys of `xread_options`, only with a `group` option. A polling or stream
  client's handler is the function that holds the call. The shared evaluation (`produce`,
  `consume`, `topic`, `topics`, `publication`, `queue_name`) lives in `channel/message.rs`, used by
  the four readers. A `let mut` bound once keeps its call as `Facts::mutable`, read by
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
  which the PyPI adapter shares); a package comes before a module of the same name.
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
  are not read, apart from item macros (`lang/rust/macros.rs`, #398): a `macro_rules!` whose
  first arm's matcher is exactly `$($x:item)*` (no separator) and whose transcriber is exactly
  `$( #[..] .. $x )*` (outer attributes only, no `#[path]`, no metavariable in them) passes items
  through. `record` reads, whatever the setting and the `cfg`, each file's definitions
  (`Facts::macro_rules`: the attributes as tokens, or `None` for any other definition, two that
  disagree, or one written inside macro tokens) and the names it invokes with braces
  (`Facts::invokes`, inside macro tokens too). `Rust::extend` gives each file the item macros it
  invokes whose every definition in the repository agrees, a line each after the features (the
  name, then the `cfg` predicates of its attributes, tab-separated); `table` reads them back.
  `bodies` parses, before the walk, the braced body of each such invocation written as `m!` or
  `crate::m!` (`invoked_name`; any other path is skipped) among the items of the file, of a
  module written in place, or among the associated items of an `impl` or a trait in either,
  over the file's bytes (`set_included_ranges`, so spans are the file's), `ITEM_MACRO_DEPTH`
  deep; a body with an error is left out. The decider reads the setting the index stored for a
  file (`files.setting`, `Engine::stored_setting`), so a function from such a body is
  function-like there. `built` gives a built invocation's body items in its place, each judged by
  its own attributes after the arm's predicates (`Built::keep`), so a `mod x;` in one is a `mod`
  declaration of the module and reaches its file. Conditional compilation (`lang/rust/cfg.rs`): `parse` skips a node whose
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
  the cold export. The decider's `function_like` parses under the setting the index stored for
  the file (`Engine::stored_setting`), and reads the nearest `Cargo.toml` files from disk for it
  only when the index has none. `lang/rust/crates.rs` (`Tree`) is the module tree behind the
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
