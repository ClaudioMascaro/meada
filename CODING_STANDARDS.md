# Coding standards

Judgement rules for writing and reviewing Meada code: what `cargo fmt`, `clippy`, the tests and
the hooks cannot check. The `code-review` skill and every per-ticket reviewer apply them to a
diff. Terms are `GLOSSARY.md`'s; the module map is `docs/agents/architecture.md`.

## Certain or absent

A link, provider or consumer Meada cannot place with certainty stays out of the map (spec #24,
user story 22). A wrong `exact` link and a provider at a wrong path are the worst failures: an
agent believes them and acts on them. A missing one is an open item that a person or `meada
decide` closes. A rule that cannot tell yields nothing, or a name match at `inferred` or
`ambiguous`; a guess never becomes `exact`, a provider or a consumer. In an export golden, read
each changed level, target, provider and consumer with `cargo run --example golden_diff`.

## Probes

A change to resolution, a contract kind or a language pack is checked against every probe that
applies to it. Each applicable probe has a fixture case in the change's tests; a missing one is
a review finding.

- **Hidden names.** Every binding form that rebinds an outer name hides it in its scope: in
  Python `with ... as`, `except ... as`, `:=`, comprehension targets, lambda parameters,
  `global` and `nonlocal`, `match`/`case` captures, an import inside a function or class,
  compound targets (unpacking, `self.x` inside one), names a closure captures; in TypeScript
  parameters, `catch` bindings and destructuring; in Rust a shadowing `let` (also inside a
  block, a loop or a closure), function, nested `fn` and closure parameters, `for`, `match`,
  `if let` and `while let` patterns, and a `let mut` or a plain assignment (a reassigned name is
  no constant). A call after the rebinding resolves through the inner binding, or by name
  match, and never to the outer definition. In Rust an inline `mod {}` is a scope boundary: its
  body sees by bare name its own items and the `use`s inside it, never the enclosing file's
  items or `use`s (those take `super::`). Values and types/modules are separate namespaces: a
  `let` or a parameter leaves a path head (`a::f`) bound as before, and a `fn` leaves a `mod` of
  the same name bound as before.
- **Opaque outside.** A base class or a receiver whose class comes from outside the repository
  stays opaque: its members resolve to nothing, and reaching it before a repository class in the
  method order leaves the member unknown.
- **Unread mounts.** A router, blueprint or URLconf whose mount Meada does not read (an import
  that does not resolve, an `include()` or `include_router` whose argument cannot be read)
  yields no provider for that mount: its routes are neither guessed at the root nor under a
  guessed prefix.
- **Name-matched hops.** A call resolved only by a name match (`inferred` or `ambiguous`) never
  feeds a contract, since it may be on any object or reach another function: wrapper expansion
  and the value flow that finds routers and clients (through constants, arguments and return
  values) climb `exact` hops alone. The one exception is a C# extension method called on a
  receiver (`x.M()`), which the resolver keeps a name match only because it does not look the
  extensions up: the value flow takes it when the static class declaring it is in scope of the
  call by its name, as the compiler binds it (`Resolver::extension_callers`; decided in
  `docs/adr/0018-csharp-extension-calls-in-the-value-flow.md`).
- **Unreadable parts.** A path or prefix with a part that cannot be read (an unfolded template or
  f-string hole, a parameter, an environment read) yields no provider, never a `{}` segment. A
  parameter written in the literal (`:id`, `{id}`, `<int:id>`) stays one.

## Hostile input

A reader of a file the repository holds (a provider document, a config, a source file) treats
its bytes as **hostile**: a crafted file ends as a `meada status` line or an open part, and
indexing carries on.

- Recursion over nested input stops at a depth limit; deeper parts are open, or the file is
  unreadable at that line.
- Evaluation visits each node once: a value read through a chain (`a().b().c()`, a `+` fold, a
  wrapper's arguments) costs the chain's length, never a factor per link, so a chain up to the
  depth limit stays cheap. A unit test feeds such a chain and bounds its time, as the
  `*_in_linear_time` tests do.
- Expansion (aliases, `$ref`s, variable substitution) is charged to a budget per file, and
  substitution runs once over the original text.
- A path read from a file or a config stays inside the repository root: relative, without `..`,
  and through no symlink.
- Each limit is a named constant with a `ponytail:` marker, listed under "Known ceilings" in
  `docs/agents/architecture.md` (`tests/it/docs.rs` checks the name is there), and a unit test
  feeds it an input past the limit.

## Known ceilings

A Known ceiling bounds only what the spec and the ticket leave out: every item they ask for is
implemented, or reported as blocked.

## Docs move with the capability

A change users can see (a language, a framework, a contract kind, a command or MCP behavior)
updates, in the same PR, the documentation capabilities page (`docs/site/pt/capabilities.md`,
with its `en` and `es` equivalents as `AGENTS.md` requires), the relevant guide or reference,
and `CHANGELOG.md` under its unreleased version.

A change to what the code relies on updates the repository map (`docs/agents/architecture.md`)
in the same change:

- A value the index now stores (a table, a column, a field of a stored summary) joins the list
  of the invariant "The index holds no source text and no values, apart from ...".
- A kind's rule lives in its own files behind `ContractKind`. The engine, the items, the
  decisions, the trace, the entry points and the extraction loop of the index writer ask the seam
  (`contract::kind_of`, `shown`, `serves`, `place` and the other free functions) and import no
  kind module; a hook added to one of them, or a kind's step added to the extraction loop, is a
  finding. What cannot be a question (a stored field, the manifest and tsconfig rows the index
  writer stores as the stored format, the phrase in the MCP `search` doc string, a variant of
  `entry::EntryKind`, a reader in `document`) is named under "Seams and what extends them".
- A package ecosystem's rule lives in its own module under `package/`, behind the `Ecosystem`
  trait in `package.rs`. The shared package steps ask the trait; the index writer asks the
  seam's free functions (`programs`, `inherit`, `is_manifest`, `read_manifest`) and imports no
  ecosystem module. Neither names an ecosystem; an arm for one added to the shared code is a
  finding.
- `jvm/` holds what the JVM ecosystems and kinds share (spec #26, user story 28): the build
  readers (`jvm/maven.rs`, `jvm/gradle.rs`), the Spring configuration reader with its injector
  (`jvm/spring.rs`) and the file readers of the JVM forms (`jvm/names.rs`). The name questions
  every kind asks of a library name (`one`, `known`, `typed`, `spellings`, `declared`, `is_method`)
  live in `names.rs`, shared with the C# kind; one a second ecosystem needs moves there, not into
  `jvm/`. An ecosystem's
  rule stays in `package/maven.rs` and a library form in its kind (`http/java*`,
  `channel/java.rs`); either one in `jvm/` is a finding, and so is a JVM reader a second
  module copies instead of sharing.
- A language pack holds syntax and value evaluation, no library table and no library form. A
  `&[&str]` table in a pack is of one of three classes: `syntax` (the language's grammar:
  `FUNCTIONS`, `DECLARATION_KEYWORDS`), `runtime` (the language's own library that value
  evaluation passes through: string methods, environment reads, standard macros, collection
  callbacks, default imports, build constraints) or `exception` (a library table the pack keeps
  for a decided reason, below). The forms of the libraries a kind reads (web frameworks and HTTP
  clients, message brokers) live under the kind, one module per language
  (`http/<language>.rs`, `channel/<language>.rs`); a form may live in a submodule of that module
  (`http/rust/frameworks.rs`). A table or a form of a library in a pack is a finding, and the
  kind owns it. Three exceptions are decided. The Rust pack folds `Method::GET` constants at
  parse time (`METHOD_CRATES` and `VERBS`) and keeps them, `VERBS` being the one pack constant a
  kind reads; the Go pack folds `net/http`'s `MethodGet`-style constants the same way, gated on
  the file's `net/http` import (`METHODS` in `lang/go/values.rs`); and the Rust pack keeps
  `ROUTE_ATTRIBUTES`, because which attributes it emits as calls that decorate a function
  (`Call::decorates`) is a syntax decision a kind cannot take after the fact without a hook from
  pack to kind. A pack may recognise the one call that reads the platform's environment, as a
  form and not a table: Java's and Kotlin's `System.getenv` (`getenv` in `lang/java.rs` and
  `lang/kotlin.rs`) and C#'s `GetEnvironmentVariable` (`ENVIRONMENT_READ` in `lang/csharp.rs`,
  which leaves the kind to decide whether the `Environment` is the framework's); C# also keeps
  the trims of `System.String` (`TRIMS`) as runtime. The test in `lang/mod.rs`
  (`every_str_table_in_a_pack_is_listed_with_its_class`) lists every such table with its class,
  so a new one is a decision visible in review: a table anywhere else in a pack is a finding.
