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

Where Meada emulates another tool through a library (Terraform through `hcl-rs`), each value the
library represents otherwise than the tool (precision, a zero's sign, a set's order, how a null
or a collection shows as text) is converted exactly or left unresolved. For Terraform,
`tests/terraform-oracle/` computes the tool's own answer with HashiCorp's HCL and cty libraries:
add the case to `cases.tsv` (one expression) or a directory under `modules/` (a variable, a
module argument, an override file) and run `go run .` there. `recorded.jsonl` beside them holds
names the tools themselves gave in `console` (Terraform and OpenTofu, run by the black-box rounds
of #754): `want` changes only with the tool run again, and a `mark` that leaves `=` is an exact
name lost, which the PR names.

When Meada emulates a tool that has a fork reading the same files (Terraform and OpenTofu), a
rule is checked against both. Where they differ Meada follows Terraform (#740), an exception
to "certain or absent" (the fork's reader may be right) that covers what Terraform reads: a
variable with a block in a file only OpenTofu reads (`.tofu`, `.tofu.json`), declaring or
overriding, has no whole Terraform answer to follow, and is unresolved where the two rules
differ; declared in a file Terraform reads, it is Terraform's reading where OpenTofu refuses the
directory (#767). The difference is recorded as a ceiling.

## Exactly N, only X

A test that proves "exactly N" or "only X" asserts over the whole output (every line of the
stream, every row of the answer), then checks each item's shape. A count over a pre-filtered
subset proves only that subset. In #451, counting the `ui: directory: unreadable` lines missed
the pre-authentication audit line a refused write also printed; the strict test counts every
`ui: ` line against reads plus picker plus writes and checks each line's shape. No lint can tell
which filter hides the criterion, so this is a reviewer's rule.

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
  method order leaves the member unknown. The one exception is a base the pack lists among its
  known outside members (`LanguagePack::known_outside`: `Serializable`, `Comparable`,
  `IDisposable`, `Hashable`), opaque only for the names it declares: for any other name its
  family resolves as one without bases (decided in `docs/adr/0019-known-outside-members.md`). A
  change to the rule has a fixture case with a listed base, a listed name and an unlisted base.
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
- **Composed operators.** A change that types a value through a new chain operator (`?`, `.await`,
  `!!`, `?.`, an unwrap) adds a test that combines it, in both orders, with every operator the
  same pack already reads. A combination the change does not type stays a name match (`inferred`
  or `ambiguous`), never `exact`: #435 typed `?` and `.await` apart, and `f()?.await.port()` with
  `fn f() -> Result<Cfg>` linked `exact` to `Cfg.port`. This is review guidance, not a mechanical
  check.
- **Same-route negatives.** A check that decides its answer by following routes (a name through
  locals, a module argument, a declared type, an override) tests the negative case on each
  route it follows: #720 kept a list exact only in a local, and a typed variable and an override
  turned a set into a list unseen.

## Equivalence tests

A test that holds two paths to the same answer (a partial read against reading every row, a
fast path against a slow one) or checks an oracle is itself checked by one mutation in the path
under test that turns it red. The PR names that mutation and the test it turned red. A switch
that changes only part of the path leaves the rest comparing the code with itself: in #396 the
`with_every_row` switch changed only `Engine::partial`, so the `search` kinds `symbol`,
`community` and `duplicate`, the contract-site spans, the `context` community line and the trace
starts could not fail. No tool checks this; a PR without the mutation is a review finding.

A case for a conversion boundary (a declared type, a module argument, an override) uses a value
the conversion changes (unsorted, with a duplicate) and a name that depends on the value; a rule
the oracle models by hand names its source, the file and function of the emulated tool, where
the model lives. In #720 and #740 one-element defaults hid a set's sort and duplicates, and an
override rule modelled from the same reading as the code agreed with it.

A new test that compares Meada with an oracle is mutated on its expected side too (add an exact
line the oracle does not give, drop one) and seen to fail, not only the code under test: in #720
the verdict skipped the exact lines beside a pattern line, and no mutation of the fix could show it.

Code that skips, caches or narrows work (a kept group, a fast path, a cache hit) also carries
one test that goes red when the path is forced off (it keeps nothing, caches nothing), because
equivalence tests pass with the optimization disabled: the answer is right, only slow. In #391
a refresh that kept no group passed every test until `a_refresh_does_not_parse_a_kept_group_again`
went red with `kept_groups` forced empty. The PR names that mutation and the test it turned red,
and a PR without them is a review finding, not a mechanical check.

## Check tests

A test that checks a rule over files, data or output (a docs check, a lint, a validator) ships
with one case that makes it fail: a fixture that breaks the rule or a mutation of the input. The
PR names that case and the test it turned red. A check that scans a set must also fail when the
set is empty. #581 shipped a check with no negative fixture test of the failure path, and the
review proved it by hand-mutating `docs/agents/map/core.md`; #595 got its `> 0` guard only after
review. No tool checks this; a PR without the failing case is a review finding.

## Hostile input

A reader of a file the repository holds (a provider document, a config, a source file) treats
its bytes as **hostile**: a crafted file ends as a `meada status` line or an open part, and
indexing carries on.

- Recursion over nested input stops at a depth limit; deeper parts are open, or the file is
  unreadable at that line.
- Evaluation visits each node once: a value read through a chain (`a().b().c()`, a `+` fold, a
  wrapper's arguments) costs the chain's length, never a factor per link, so a chain up to the
  depth limit stays cheap. A unit test feeds such a chain and bounds its time, as the
  `*_in_linear_time` tests do. Its input is sized so the linear path runs in under a twentieth
  of the bound in a local debug build (CI runners are several times slower and run the suites in
  parallel), and is also large enough that a quadratic variant of the code under test exceeds
  the bound. The implementer reports the measured time, and the PR says which mutation was
  tried and how long it took. A new language pack adds its row
  to `chained` in `tests/it/hostile.rs`, which feeds every pack a chain of locals on call
  results.
- Expansion (aliases, `$ref`s, variable substitution) is charged to a budget per file, and
  substitution runs once over the original text.
- An estimate that gates evaluation (`iac::cost`) is an upper bound of what evaluating makes.
  Each charge says in a comment why it covers the real output. A change to one is reviewed by
  hunting the cheapest input it would let past its ceiling: an expanded argument (`f(x...)`), a
  value shown longer than its counted bytes (a number), an element that costs nothing (a
  template of no text), a literal costed once. A unit test feeds such an input and expects the
  refusal, and a change that tightens a charge also tests that an input the old charge accepted,
  and Meada evaluated right, is still accepted (#716, #764).
- A path read from a file or a config stays inside the repository root: relative, without `..`,
  and through no symlink.
- Each limit is a named constant with a `ponytail:` marker, listed in
  `docs/agents/ceilings.md` or a part of `docs/agents/ceilings/` (`tests/it/docs.rs` checks the name is there, that a `const` named
  `MAX_*`, `*_MAX`, `*_LIMIT` or `*_CAP` carries the marker, and that no two share a name), and a
  unit test feeds it an input past the limit.
- A manifest or configuration reader reads a file of `lang::MAX_BYTES` built to explode (many
  keys, deep nesting, wide fan-out) in bounded time and bounded memory, and returns a bounded
  size; a value is copied after it is charged, never before. A new reader, or a reader that
  gains a table or key, adds or extends its row in `probes` in `src/package/hostile.rs` (which
  also bounds the heap each row holds, `HEAP`), so the probe exercises the new keys at
  `lang::MAX_BYTES`, including a scan of every entry for each entry.

## Service deadlines

Every call to an external service (Postgres, Valkey, the store, GitHub, an identity provider) on
a request path, in a poll or in a job runs under a client-side deadline. Background and
retryable work is not exempt: a call that never returns holds the job, and the lock it took,
forever.

- Postgres bounds its call by `postgres::Config::bound(n)`, with `n` the statements it runs
  (a transaction's begin and commit count).
- Outbound HTTP goes through `modules::agent(timeout)` with the module's configured timeout.
- Other async calls (Valkey, the store, status probes) go through `modules::within(bound, ask,
  timeout)`, where `bound` is the duration and `timeout` builds the timeout message.
- A new service brings its bound with it. A call outside a deadline is a review finding.
- A new failure answer from a service inside a loop over candidates (installations, credentials,
  peers) says whether it ends the loop, passes to the next candidate or fails the run, and a test
  asks the second candidate.
- A header with more than one form (`Retry-After`: delay-seconds or HTTP-date) is parsed in every
  form or read by presence.

## Waits inside one process

Work that waits for other work of its own process (a refresh in flight on another thread) waits
with no timer: on a `Condvar`, a channel or a join, or, where the wait goes through a timed
mechanism (SQLite's busy handler), for as long as that work is in flight. A timeout bounds only
a wait on another process or a service, whose progress this process cannot see.

In #704 the calls of one `meada mcp` server waited for its own refresh on SQLite's 10 s busy
timeout: a refresh longer than the timer failed every call behind it. One fix moved the
refreshes to a `Condvar` (`FLIGHTS` in `src/index.rs`) but left the statements of a call's open
and queries on the timer, and the same reply came back until they got a busy handler that waits
while a refresh is in flight (`wait`). A wait that breaks this is a review finding; the ones
#704 left are in the ceilings entry "Concurrent refreshes of one index (#704)".

## One decision, one function

A decision that more than one surface makes (console, node, CLI, browser), such as access,
visibility or text validation, is one function in `meada-core`. Every surface calls it; the
browser reaches it through the WebAssembly export (`meada-core/src/wasm.rs`), needed only when
the browser decides too.

- A second copy of such a decision, in TypeScript or in another Rust module, is a review finding.
- The instances today: `meada_core::effective::effective` (the console calls it and the browser
  reaches it through the export) and the functions in `meada_core::named` (the name, description
  and message checks, and the access helpers such as `visible`).

## Known ceilings

A Known ceiling bounds only what the spec and the ticket leave out: every item they ask for is
implemented, or reported as blocked.

A documented class (a ceilings entry, a doc comment, a test name) is worded as narrowly as its
tests: it lists the syntactic positions they cover, or they cover every position its wording
names. #718 wrote "`*`, `+` or `?` with no operand" for three tested positions; after a group
name or a group of flags Alertmanager refused the file and Meada kept its links `exact`.

## Claims the diff adds

Check each claim a diff adds at its source, never against the ticket's wording:

- A citation (a file, a constant, a test, "X derives from Y") is opened and confirmed at the
  cited place.
- A rule is checked by asking which action clears it: read the code or the test that produces
  or guards the artefact (for a `docs/agents` file, the test in `tests/it/docs.rs` that names
  it). "Re-bless" with no producer found, or a sentence placed where that test expects other
  text, is a finding.
- A cause or a cost (a doc, a ceilings entry or a PR body saying why a run failed, or how much a
  change costs) is traced to the run or the test that isolates it: the verifier's report, a
  measurement with its commit, build, case and numbers. One that nothing isolates is worded as
  a reading of the code. A draft of #704's PR body, and its ceilings entry, gave as fact the
  cause of a failed run whose report named two candidate paths and confirmed neither.
- A reference the diff moves or reflows still sits next to the thing it names.
- A change to a skill or a harness script searches the skill's own texts for the behaviour it
  changes; a ticket premise they contradict is a finding.

## Docs move with the capability

A change users can see (a language, a framework, a contract kind, a command or MCP behavior)
updates, in the same PR, the documentation capabilities page (`docs/site/pt/capabilities.md`,
with its `en` and `es` equivalents as `AGENTS.md` requires), the relevant guide or reference,
and `CHANGELOG.md` under its unreleased version. A change to what a pack or reader links
updates `docs/site/*/capabilities.md` and `docs/user-guide{,.en,.es}.md` together.

A change to what the code relies on updates the repository map (`docs/agents/architecture.md`)
in the same change. Each kind of update goes to one place, and the map stays an index within its
budget of 8 KiB (`tests/it/docs.rs` fails above it). The contracts and resolution/index/shape
ceiling indexes share that entry budget; their topic leaves and the relocated map details stay
within 24 KiB. Other areas keep their existing 64 KiB budget until split:

- The map holds a line for a module, a seam or an invariant: what it owns or extends, in a
  sentence, and the area file that has the rest. A new module gets its line, naming its area
  file (`tests/it/docs.rs` checks every top-level file and directory of `src/` and
  `meada-core/src/` has one).
- The detail behind a line (behavior, routes, store functions, per-language and per-library
  entries, the steps of a seam too long for the map) goes to the area file in `docs/agents/map/`,
  a new one when none fits. A paragraph that only repeats an ADR or a site page links to it.
- A limit goes to the part of `docs/agents/ceilings/` for its area (listed in
  `docs/agents/ceilings.md`), with its value and its reason; it is not repeated in the map.
- A value the index now stores (a table, a column, a field of a stored summary) joins the list
  "Stored values" in `docs/agents/map/indexing/stored-values.md`, the detail of the invariant "The index holds
  no source text and no values of code, apart from ..." in the map. A diff that adds a form to a
  stored field and leaves `tests/golden/schema.txt` unchanged is a finding
  (`docs/agents/map/seams.md`: a change to what is stored re-blesses it). The golden is built
  from the samples under `tests/fixtures/schema/` (`src/index.rs`), so the fix is a sample there
  that carries the new form, added or extended in the diff, and a re-blessed golden that shows it.
- A kind's rule lives in its own files behind `ContractKind`. The engine, the items, the
  decisions, the trace, the entry points and the extraction loop of the index writer ask the seam
  (`contract::kind_of`, `shown`, `serves`, `place` and the other free functions) and import no
  kind module; a hook added to one of them, or a kind's step added to the extraction loop, is a
  finding. What cannot be a question (a stored field, the manifest and tsconfig rows the index
  writer stores as the stored format, the phrase in the MCP `search` doc string, a variant of
  `entry::EntryKind`, a reader in `document`) is named under
  [Seams and what extends them](docs/agents/map/overview/seams-and-what-extends-them.md).
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
  `&[&str]` or `[&str; N]` table in a pack is of one of three classes: `syntax` (the language's grammar:
  `FUNCTIONS`, `DECLARATION_KEYWORDS`), `runtime` (the language's own library that value
  evaluation passes through: string methods, environment reads, standard macros, collection
  callbacks, default imports, build constraints) or `exception` (a library table the pack keeps
  for a decided reason, below). The forms of the libraries a kind reads (web frameworks and HTTP
  clients, message brokers) live under the kind, one module per language
  (`http/<language>.rs`, `channel/<language>.rs`); a form may live in a submodule of that module
  (`http/rust/frameworks.rs`). A table or a form of a library in a pack is a finding, and the
  kind owns it. Six exceptions are decided. The Rust pack folds `Method::GET` constants at
  parse time (`METHOD_CRATES` and `VERBS`) and keeps them, `VERBS` being the one pack constant table
  a kind reads, besides the TypeScript pack's `GLOBAL_URL`, the callee it gives a two-argument
  `new URL` of the runtime global, which the file's own declarations decide; the Go pack folds `net/http`'s `MethodGet`-style constants the same way, gated on
  the file's `net/http` import (`METHODS` in `lang/go/values.rs`); and the Rust pack keeps
  `ROUTE_ATTRIBUTES`, because which attributes it emits as calls that decorate a function
  (`Call::decorates`) is a syntax decision a kind cannot take after the fact without a hook from
  pack to kind; the TypeScript pack keeps `RESPONSES` (`Promise`, `NextResponse`) and `INFERS`
  (`infer`, `output`, `TypeOf`) in `lang/ts/types.rs`, because it unwraps a handler's declared
  return type at parse time to reach the schema a type infers (`z.infer<typeof S>`, io-ts
  `t.TypeOf<typeof S>`), which no kind can do after the fact, and those are the words that walk
  reads. The Go pack reads the `env:"X"` and `envconfig:"X"` struct tags as environment reads
  (`tagged` in `lang/go/values.rs`, #618), and the Python pack reads the fields of a
  pydantic-settings `BaseSettings` class as environment reads (`settings_consts` in
  `lang/python.rs`, #619), because the read happens where the field is selected, which no kind
  can see after the fact. A pack may recognise the one call that reads the platform's
  environment, as a form and not a table: Java's and Kotlin's `System.getenv` (`getenv` in
  `lang/java.rs` and `lang/kotlin.rs`) and C#'s `GetEnvironmentVariable` (`ENVIRONMENT_READ` in
  `lang/csharp.rs`, which leaves the kind to decide whether the `Environment` is the
  framework's). The TypeScript pack also keeps Backstage's `config.getString('a.b')` and
  `getOptionalString` as an `exception` table (`CONFIG_READS` in `lang/ts.rs`, #617): the call
  is a config read the kind (`http/ts/config.rs`) looks up in `app-config*.yaml`. C# also keeps
  the trims of `System.String` (`TRIMS`) as runtime. The tables of known outside members
  (`LanguagePack::known_outside`) are runtime, apart from Swift's entry for Vapor's
  `RouteCollection`, an exception decided in ADR 0019. The test in `lang/mod.rs`
  (`every_str_table_in_a_pack_is_listed_with_its_class`) lists every such `&[&str]` table named
  by a `const` or `static` with its class (a `known_outside` table is a literal its method
  returns), so a new one is a decision visible in review: a table anywhere else in a pack is a
  finding.
