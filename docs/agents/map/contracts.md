# Contract kinds

Detail behind the [architecture map](../architecture.md), which keeps the flow, identities,
seams, invariants and Known ceilings. The code wins over this file, and a change to these
modules updates it.

- `contract.rs`: `ContractKind` trait (`prefix`, `extract`, `declare`, `sides`, `link`),
  `KINDS`, `Level`, `Link`, `Names`, `Role`. A `Found` carries its `role` (`Provider`,
  `Consumer`, `Binding`, `Publish` or `Import`),
  stored in `contracts.role`; the site does not imply it (a channel producer is a `Site::Call`, a channel
  consumer a `Site::Def` or `Site::Inline`). A `Site::Def` name is folded through `Names::scope`
  in the index: unnamed inline segments drop, and an empty result is the file's top level. `http/ts/clients.rs`: `fetch` consumers (also a call to a parameter defaulting to
  `fetch`; only in a pack whose `globals` has it; each with `Found::shape` from `response_reads`, spec #23: over the frames
  `Code::expand` climbed, the field paths read from the `.json()` of the response in the
  function making the `fetch` and in each wrapper up to the site, a wrapper passing on the
  response, the body or a path below it when it returns that one thing from the function its
  caller reaches), Python client consumers (`http/python/clients.rs`, below), Next.js app-router
  and Express (`http/ts.rs`), FastAPI, Flask and Django (`http/python.rs`), axum
  (`http/rust/axum.rs`) and actix-web and Rocket (`http/rust.rs`) providers (Next.js and
  Express ones each with a `Found::shape` from `http::ts::provider_shape`, the one place that picks a route's code source: `zod::Zod::returned`
  first, else `literals::Literals::shape` (spec #23; both read the 2xx bodies `provider_shape`
  lists once with `Literals::responses`): one `Literals` per `extract` (with its `Types` and their
  `Zod`, which `response_reads` and `provider_shape` reach through it), holding the budgets of
  each file; Express reads a handler once for every prefix it is mounted under),
  `normalize_path`, and the link rules (host,
  `base_url_env`, unique provider). A provider that serves every method has the method `ANY`
  (`http::ANY::<path>`): `http::fits(provided, wanted)` matches it to a consumer of any method
  on its path, and its link's reason ends with `; any method`; `route_fits` and a `trace` start
  (`POST /health`) take it the same way. `http::drift(links, exports)` sets `Link::drift` on every
  HTTP link but an ambiguous one whose consumer (by site and the link's contract, or by site
  alone for an unresolved one; several of one contract count only when their shapes agree) and
  provider (by site, serving the link's contract) both have a shape, through `shape::drift`. An Express route path or mount prefix with a part that cannot
  be read (a template-literal hole, a parameter, an environment read) adds no route: a router
  mounted only under such a prefix is served under no prefix, never at the root; a literal `:id`
  stays a parameter. `declare` first adds the routes of the repository's
  documents (spec #21): a document route whose contract a code provider already has merges
  into it (one provider, the code's handler site); any other becomes a provider of its own at
  `<repository>::<document path>::<METHOD> <path>` (no handler), and a route an earlier
  document by path listed adds nothing. Each provider of a documented contract, merged or not,
  takes `Provider::shape` from the first document by path that lists it, when that
  document's route has one (else the provider keeps the shape its code gives, if any; none when that
  document's route has no shape). A document provider is a written contract: it lowers
  no level (a host match is `exact`). A route also matches by its gateway paths: `declare`
  fills `Provider::gateways` (`Gateway {path, reason}`, not exported; the `[serve]` form, which
  is also the exported `Provider::gateway`, first, then the forms of each document listing the
  route: one per server path of the document, with `[serve] strip`, or the `[serve]` form when
  the document has no server path; a code route always has its `[serve]` form, a route only
  documents list has it only through a document without server paths), `Provider::hosts` (not
  exported: `[serve] hosts` plus the host of every `servers` URL of the repository's documents,
  checked by `link` for a `host:` consumer). `shape`, `gateways` and `hosts` are set on HTTP
  providers only; `http::gateway_contracts(provider)` yields each as
  `(contract, reason)` and `http::serves(provider, contract)` is the one check of any path, through `fits` (also
  used by open items and decision replay, so a gateway candidate carries the provider's own
  contract). That link is `inferred` with the form's reason (`gateway prefix <p>`, `gateway
  strip <s>` or both), read from the provider, not recomputed from the configuration; within
  one repository a code path match wins over gateway path matches. An app or router
  (Express `express()`, `Router()`; FastAPI `FastAPI()`, `APIRouter()`; Flask `Flask()`,
  `Blueprint()`) is the call creating it (axum's `Router::new()`, read by `http::rust::axum`: `route`,
  `nest` and `merge` calls on a receiver chain that `Code::origins` follows through
  `Call::receiver` where `LanguagePack::fluent` is true, a method router `get(a).post(b)` or
  `any(h)` read by `method_router`, a closure handler an `Inline` def, the paths normalized by
  `normalize_path`, with `{{` and `}}` of axum 0.8 kept literal, and the method the name axum gives the function; the Express rules do not run on `.rs` files; `nest` prefixes and `merge`
  go through `prefixes` as Express mounts do, a router given to a call Meada does not read (`route_service`,
  `fallback_service`, an outside wrapper, a name-matched method; `axum_unread`) is mounted
  nowhere, a mount through a value it cannot read serves nothing, and a router nothing nests is
  served at its root; `prefixes` stops at `PREFIX_DEPTH` and `PREFIX_WORK`); actix-web and Rocket (`http/rust.rs`) reuse these seams: a handler is a function a route
  attribute sits on (`#[get]`, `#[route(path, method = "X")]`; the pack emits it as a `Call` with
  `decorates`) or one `.route(path, web::get().to(h))` names; an `App`, `web::scope`,
  `web::resource`, the `ServiceConfig` a `.configure(f)` function receives or a Rocket instance
  is a mount node, a handler is one too. A handler any call names (`.service(h)`,
  `routes![a, b]`) is served only where a mount Meada reads puts it, one no call names at its own
  path; a scope or a mount prefix Meada cannot read serves nothing (never the root). `routes![..]`
  is the one provider-side macro read; each handler in it resolves exactly through `use`, or yields nothing (`Code::external_of` reads the outside name of a callee, a path call's head among types and modules, so a `fn rocket()` does not hide the crate in `rocket::build()`; a bare route attribute comes from the `#[macro_use] extern crate` of its crate's root; `Code::mentions` tells whether a file refers to a name, which, over the handler's crate (every crate when `Code::published` says its file may be imported), decides whether an unregistered handler is served at its own path), reached through `Code::origins`: a route counts only
  on a receiver that traces back to its framework's constructor (`Code::external`; Express
  also takes an unimported or local name), so `@app.get` on a Flask app yields nothing. Each
  route is served under every prefix (`prefixes`): the segment of each mount (`use`,
  `include_router`, `mount`, `register_blueprint`; Python's in `Framework::mounts`) under every
  prefix of the router it is mounted on; FastAPI's mount segment is the given prefix (or
  `mount`'s path) then the router's own (`APIRouter(prefix=)`), Flask's given `url_prefix`
  replaces the own one (`Blueprint(url_prefix=)`). What nothing mounts is served at its root
  (`prefixes`' `root`): every Express app or router; a FastAPI or Flask app, unless a mount
  whose child cannot be read (no origin of the child is an app or router of the framework),
  called on another app or router of its framework and able to hold an app (FastAPI's
  `include_router(app.router)` and `mount`, not Flask's `register_blueprint`), may mount it;
  never an `APIRouter` or a `Blueprint`, which serves only where a mount Meada reads puts it,
  so a router mounted through an import that does not resolve is not guessed at the root. A
  prefix or a path with a part that cannot be read (an f-string or template-literal hole, a
  parameter) adds no route (Express included: `` `/${name}/items` `` is no provider, and a
  router mounted only under such a prefix is served under no prefix); a path parameter written
  in the literal (`{id}`, `<int:id>`, `:id`) stays one. FastAPI method
  decorators and `api_route`, and Flask `route`, are read from `Call::decorates`: the handler
  is the decorated function, the methods `methods=[...]` or `GET`. Django (`django`, `http/python.rs`): what an
  `include()` argument names is an `Include`: a module of the repository (by dotted name or
  as a module), the entries of a list, either of them first in the tuple form `(module or
  list, "app_name")`, a dotted name no module answers to from the source roots (an import
  from outside the repository, or one that does not resolve: `Named`), or `Unknown` (anything
  else, or a value whose origins are not all `path()`/`re_path()` entries). A root URLconf is
  a file with a module-level `urlpatterns` that no `include()` may name: none resolves to it,
  no `Named` is its dotted path or a suffix of it (`may_be`: `backend/shop/urls.py` may be
  `shop.urls`), and no `include()` is `Unknown` (then there is no root). Its `path()` and
  `re_path()` entries (`Code::origins` of `urlpatterns`) are walked top-down, an `include()`
  view adding its route before the entries it holds, at any depth, an `include()` met again
  on the walk (a cycle) adding nothing, every entry read charged to `URLCONF_BUDGET`. Routes join as Django joins them (plain concatenation). A
  `re_path()` pattern is read only when it holds literal text and `(?P<name>...)` groups
  (`regex_route`); any other yields nothing. A function view (`Code::definition`) serves what
  `require_http_methods([...])`, `require_GET`, `require_POST` and `require_safe` allow (from
  `Call::decorates` and `Def::decorators`, all of them when several restrict it), else `ANY`;
  `C.as_view()` serves each handler method `C` itself defines (`get`, `post`, ...), each one
  the provider's site. Python clients (`consumers` in `http/python/clients.rs`, from `LIBRARIES`: `requests`, `httpx`,
  `aiohttp`): a call is a consumer when `Code::external` names it a module function of a
  library imported by the file (`requests.get`, `httpx.post`, also under an alias), or when it
  is `get`, `post`, ..., `request(method, url)` on a receiver that `Code::origins` traces back
  to a constructor of the library (`Session()`, `Client()`, `AsyncClient()`,
  `ClientSession()`): assigned in the scope, bound by `with`, a module name, a `self`
  attribute, or a parameter the callers pass one to, or a value a function of the repository
  returns. This is wider on purpose than the three binding forms spec #24 names (same scope,
  `with`, a `self` attribute), as the final review accepted: every way `Code::origins` follows
  a value counts (module constants, caller parameters, return values). A `get` on any other
  object is nothing. Rust clients (spec #30 #131) are the same code with two more `LIBRARIES`
  entries: `reqwest` (modules `reqwest` and `reqwest.blocking`; clients `Client::new`,
  `Client::builder()`, `ClientBuilder::new` and the `blocking` equivalents, a builder chain
  followed to its constructor by `fluent`, and every method on the way listed by `passes`:
  any other method on an outside value ends the chain) and `ureq` (module functions; clients `agent()`,
  `Agent::new*`, `AgentBuilder::new`). The receiver of a chain (`Client::new().get(..)`,
  `builder.build()?.get(..)`) is `Call::receiver`, read by `Code::value_origins`. The method
  is the name, or `request(Method::X, url)` (reqwest: a `Method` of `reqwest`, `http`, `hyper`,
  `axum` or `actix_web`, written in full or imported with `use`) and `request("X", url)`
  (ureq). `lang/rust/values.rs` folds the URL like any `Value`: literals, `+` (one side a
  string), `String::from`, `.to_string()`, `.to_owned()`, `.as_str()`, `format!` and `concat!`
  (the format string, `{}`, `{0}`, `{name}`, `{name}` captures, `name = value` arguments and
  `{{`/`}}`; an argument that is a literal or a plain name folds, any other is a `Part::Hole`,
  and so is a format spec such as `{:?}` or `{x:>8}`; a `format!` the file defines itself is
  unknown), and `lang/rust/builders.rs` a `let mut` String whose block only reads it and
  appends with `push_str`, `push` and `+=` statements: a read holds the text of the pushes
  before it. An environment read is a `Part::Env` (`Reader::env_read`): `std::env::var("X")`,
  `dotenvy::var("X")` (spelled in full or through the one `use` of their head, `Reader::spelled`)
  and `env!("X")` (also as a `concat!`/`format!` argument), through `ENV_METHODS` (`unwrap*`,
  `expect`, `?`, `trim*`, `to_string`...). An argument that mentions `env`, `var` or `dotenvy`
  (a second read in a fallback) makes it a `Part::Hole`; `option_env!` and an `env` the file
  defines are no read. A leading `Part::Env` is `via = env:X` in `http::locate`.
  `client_base` reads the constructor's `base_url=` and `based` joins it, with one `/`, in
  front of a relative URL (an absolute one, or one starting with a hole, is left alone);
  different constructors with different bases make the base a hole. URL and method fold like
  any `Value` (`str.format` and `%` do not). `python.rs` folds an environment read into
  `Part::Env`, as `ts.rs` does (`env_read`/`single_env`): `os.environ["X"]`,
  `os.environ.get("X")`, `os.getenv("X")` and the bare `environ`/`getenv`, matched as written
  (not through the file's imports), alone or in an f-string hole, through `or` fallbacks (the
  read on the left, no other variable on the right), a default argument (ignored, unless it
  reads another variable) and string methods (`strip`, `rstrip`, `replace`, ...). A leading
  `Part::Env` is `via = env:X` in `http::locate`, linked through `base_url_env`; Meada never
  reads the value. `Code::expand` follows a function whose URL
  or method depends on its parameters, so each of its call sites is a consumer (an argument
  it leaves out takes the parameter's default). In Express `use(first,
  ...)`, `first` has no path only when it is a router (a call, also one `Code::origins` reaches
  through arguments and return values) or a handler (a function or a definition); anything
  else is an unreadable prefix, so what it mounts is served under no prefix. `resource.rs` (spec
  #19): reads no code; `declare` turns each `[[resource]]` `via` symbol the repository has into a
  provider (writer) or consumer (reader) of `resource::<kind>::<id>`, and `link` joins every
  reader to every writer of another repository at `declared`, reason the contract. A `via` that
  names no symbol is dropped. An entry without `via`, or with none matched, makes the repository
  the side: the `site` is the bare repository identity (no `::`), the **repository-level** side.
  `report` gives the status lines per repository: each unmatched `via`, the fallback to repository
  level, and a resource with no link in the workspace (unlinked). `channel.rs` (spec #20):
  `extract` calls `code.pack().messages(code)`; a producer is a provider at its call site, a
  consumer a consumer at its handler (an inline handler is named `<broker> <names in order>`,
  such as `kafka orders.created`). `contract(broker, name)` is `channel::<broker>::<name>`, or
  `channel::<broker>` for an unresolved side (`channel::parse`, `is_unresolved`, and `shown`, which
  prints it with `(unresolved)`), which `link` skips: it is in the export and never
  linked, and since its contract is not `None` it is no open item (`search` kind `unresolved`
  and `meada decide` never see it). `link` joins every consumer to every producer that
  `reaching(contract, providers, routes)` gives, in any repository including its own. Name
  matches are `fits(produced, consumed)`: the same contract at `exact`, reason the contract,
  and a consumer whose name is a wildcard pattern to the producers its pattern matches
  (`wildcard(broker, pattern, name)`, the one place a broker's matching rules live: NATS `*` one
  token and `>` the rest, a subject holding a wildcard matching nothing, table-tested), at
  `inferred`, reason `<producer contract> wildcard <pattern>`. RabbitMQ (`amqp`): a binding is a
  `Found` with role `Binding`, stored in `contracts` (`contract` the queue's channel, `via` the
  exchange, `method` the pattern, `site` the `bindQueue` call) and exported as `bindings`
  (absent when empty). A publish to a named exchange holds no channel: it is a `Found` with
  role `Publish` and no contract (`via` the exchange, `method` the routing key, `site` the
  `publish` call), exported as `publishes: [{exchange, key, site}]` (absent when empty), so no
  queue named like its exchange or its key ever equals it; a publish to the empty exchange is
  a plain producer of the queue `key`, and one whose key cannot be evaluated an unresolved
  producer `channel::amqp`. `Routes` holds the workspace's bindings and publishes (the broker
  is shared); `routed(publish, binding)` is the one routing rule: same exchange and the
  pattern `pattern_matches` the key (topic-exchange rules whatever the exchange type), `exact`
  when the pattern is the key, else `inferred`, reason `<queue contract> binding <exchange>
  <pattern>`. `reaching` adds those publishes to a queue's producers, and `Routes::reached`
  gives the queues one publish reaches; of several matches to one producer the strongest wins.
  `ContractKind::link` takes the workspace's `Routes` beside its providers. `declare` adds the
  channel sides of the repository's AsyncAPI documents (`Summary::channels`, in path order): a
  document side whose contract equals a code side's of the same direction merges into it (every
  such code provider or consumer keeps its site and takes the document's shape unless it has
  one), any other is a side without a handler, a provider at `<repository>::<document>::<broker>
  send <name>` or a consumer (`via` null) at `<repository>::<document>::<broker> receive
  <name>`; a side an earlier document by path already added merges the same way, so adds
  nothing. Both carry `shape` (`Consumer::shape` is new, exported when present). `link` is
  unchanged by this: the documented sides are ordinary providers and consumers, so a documented
  producer reaches the code consumers of other repositories at `exact`, and a code producer
  reaches the documented consumer (an `amqp` side of a document is a queue name, never an
  exchange, so it is never a publish). It sorts `providers` and `consumers` into the index's
  order afterwards.
- `package.rs` (spec #22): packages as contracts. `read(bytes)` turns a `package.json` into a
  `Summary {name, version, dependencies: [Entry {field, name, range}], exports, types, module,
  main}` (the four dependency fields, sorted by field then name; `exports` as written, `null`
  counting as absent) and never fails: a file over `MAX_BYTES` (1 MiB), not a JSON
  object or nested past `serde_json`'s depth limit reads as empty, a field of another type or an
  empty string counts as absent, and a name that is empty, over `MAX_NAME` (214), or holds a
  control character, whitespace or `:` is no name (a dependency entry with such a name is dropped).
  The kind `Packages` (`package::npm::<name>`, sides `package` and `dependent`) `extract`s an
  `Import` row (`Role::Import`, never exported) for each call whose callee's head names an import
  of a bare specifier from outside the repository (`Code::imported`: specifier in `via`, the
  chain spelled from the imported name in `method`); `declare` makes a provider at `<repository>::<manifest path>` (`Provider::version` for the
  reason) and a `packages` row `{name, version, repository, directory}` (`.` at the root) for each
  named manifest, private ones too. `consume(exports, providers)` is the workspace-wide step `Engine::exports`
  runs after every repository's `declare` (one repository cannot see another's providers; the
  `Providers` index of package names to their manifests is built once there and shared with
  `import`, and `reaches` is the one place the `workspace:` rule lives): for
  each dependency entry whose package some repository of the workspace provides (a `workspace:`
  range needs one in its own repository), it adds a consumer at `<repository>::<manifest
  path>::<field>::<name>` (the range travels in `Consumer::method`, which is `serde(skip)`) and a
  `dependencies` row `{manifest, field, name, range}`. An entry on a package outside the workspace
  appears nowhere. `link` gives one candidate `exact` and several `ambiguous` (one link per
  candidate, `workspace:` keeping the consumer's repository), with the reason `<identifier>
  <range|none> (local <version|none>)`, prefixed `<n> providers fit; ` when ambiguous.
  `reason()` is the one place it is built; it ends with `, range mismatch` or `, range
  unchecked` when `flag(range, version) -> Option<Flag>` (public; `context` calls it too, and
  nobody parses the reason back) says so. `flag` checks with `nodejs-semver` (npm syntax: `^ ~
  x - ||`, prereleases as npm): `Mismatch` when the range does not accept the local version,
  `Unchecked` for a missing range or version, a `workspace:`/`file:`/`link:`/`npm:` range, a git
  or URL source, a tag such as `latest`, a range past `MAX_RANGE`, and any range with a token
  that is not a comparator (the crate parses `foo 1.2.3` as `1.2.3`). The flag is computed at
  query time and stores nothing. `import(exports, providers)` is the second workspace step, after
  `consume`: for each import row whose package the importing file's nearest manifest
  (`nearest`: the deepest directory holding it, named or not) depends on in any field, it
  resolves the call in every provider of that package (a `workspace:` range: its own repository
  only) through `Provided::resolve`: `ways` gives the ways in, each with the files it names and
  its `How` (`exports` through `exported`, the table-tested lookup of spec #22: exact key, then
  the `*` pattern with the longest prefix, the first of `types`, `import`, `default`, `require`
  at each level, `null` blocking, a key or a target with two `*` not read, the only way in when
  present; without `exports`, for the bare name one way for each of `types`, `module`, `main`
  present, tried in that order (an empty or non-string `types`, `module` or `main` counts as
  absent), or, with none of the three, `index.<ext>` for each pack extension, as Node's default
  does (`path`, never `index` alone or `index/index.<ext>`); for any other subpath, the subpath
  itself, as a path with the trunk's extension and `index` rules, `path`). A file must stay
  inside the package directory (`inside`), be a source file the provider indexes, not a
  `.d.ts`, and have this package's manifest as its nearest (`owned_source`: a file of a nested
  package is not the outer package's); the first way that leads to one, directly or through the
  mapping, wins, so a field that leads nowhere does not stop the search. One that is build output
  maps back to source: `mapped` takes the `tsconfig.json` in the package directory; only when that
  file is absent or invalid (stored as `null`, no `Config`) the repository root's, and a valid
  one without a usable `outDir` maps nothing, with no fallback. `roots` turns its `outDir` and `rootDir` (relative to that config;
  `rootDir` defaults to the package directory) into repository directories, and `sources`
  (the pure, table-tested mapping) swaps the entry's `.d.ts`, `.js`, `.mjs`, `.cjs` or `.jsx` for
  each pack extension in `EXTENSIONS` order under `rootDir`; the first the provider indexes wins,
  `inferred` with `how` `outDir`. When no way finds a file, directly or through the mapping, the
  import is `ambiguous` and `how` names the declared field or rule whose entry could not be
  followed to a source file (the first way tried: `exports`, the first of `types`, `module`,
  `main` present, or `path`): `candidates` gives the
  symbol the chain names in every source file whose nearest manifest is this package's (not a
  nested package's, not one under a `node_modules` segment, not a `.d.ts`) and that exports its
  head (sorted, one hop each; none, nothing). The files by owning manifest are grouped once per
  provider (`Provided::owned`), not once per call. `lookup` then reads the chain's
  head from the `exports` rows (the file's own export of the name decides; else its `export *`
  files, never for `default`, two that give it differently making it ambiguous and so not
  exported, as in ES modules; `seen` cuts cycles) and each further name as a member of that
  definition or a name of a whole file. A `tsconfig.json` has a row in `manifests` too, read
  by `read_tsconfig` (`out_dir`, `root_dir`; `Export::tsconfigs`, `serde(skip)`; `extends` is not
  followed, as the trunk follows none), so an edit relinks like a `package.json` one. `import` skips a specifier no repository provides before
  reading anything else, and builds a provider's tables (`Provided`) the first time an import
  reaches it; `index::export` reads the `import` rows only for a repository with a manifest and
  the `exports` rows only for one with a named manifest. A call resolved in one provider at least
  becomes a
  consumer of the package at its call site (range in `Consumer::method`, where it resolves in
  `Consumer::hops`, both `serde(skip)`), and each resolution an `imports` row `{site, specifier,
  name, level, how, target}`. `link` then gives a call with hops, instead of the manifest, the
  symbol in each provider whose package link it would have, at the weaker of the two levels,
  with the reason `<package reason>; import <how>` (`How::name`: `exports`, `types`, `module`,
  `main`, `path` or `outDir`; on an `ambiguous` link, the field or rule whose entry could not be
  followed to a source file). The hop is derived at query time from stored facts and never stored; an import of a
  package outside the workspace, or that resolves nowhere, appears in no export. A re-export of
  another package inside the provider ends the lookup (no second hop). No lockfile is read and
  no registry queried. Shapes cross the same way (issue #175): `surface(manifests, configs,
  exported, files)` is the public surface, the entry files of each named manifest's bare name
  and `exports` subpaths without a `*` (`Provided::entry`, the part of `resolve` that follows
  the ways in) and every file their `exports` rows name, in turn; `exported_shapes(code,
  surface)` (run by `index::refresh`) gives, for the files of the surface, a `Role::Shape` row (`site`
  the file, `method` the name, `via` the `shape::Want`) for each module-level constant or type
  a file exports: `Types::exported` for an interface or alias (`type`), `Zod::exported` for a
  schema (`zod` and `infer`), else `Literals::exported` for an object or array literal
  (`literals`); `index::export` loads them as `Export::shapes` (`ExportedShape`). `shapes(exports,
  providers)`, the third workspace step, fills every provider and consumer shape holding a
  `shape::Import` through `shape::fill`, whose lookup `kept` resolves the import like a call
  (`nearest` manifest of the importing file, its dependency entry, `reaches`, then
  `Provided::resolve` with the chain the import names, `known` being the kept shapes of the
  import's view instead of the symbols) and gives the resolution's level (the weaker of two
  providers keeping the same shape); none, `ambiguous`, several names or two providers keeping
  different shapes is unresolved. A consumer left with no field has no shape.
  `Resolver::exported` names constants and types as targets too, which no call reaches since
  they are no symbols.
  Go (spec #25): `package/go.rs` reads a `go.mod` (module path, no version; each `require` an
  entry under `require` with the version as range; a `replace` to a local directory gives its
  entry, or one under `replace` when nothing here requires the module, the Cargo `path` form in
  `via`; a `replace` by anything else marks the entry `outside`, which reaches no provider). A call
  from a `.go` file through an import of a module the nearest `go.mod` requires (a path with no dot included) resolves in the provider
  directory the import path names (`Provided::package`): the exported name of the one non-test
  source file of that directory (nested modules left out) that defines it, `exact`, `How::ModulePath`.
  Ecosystems (spec #24): everything above is npm's, behind `Ecosystem` (`Npm`, `Pypi`), which
  names a manifest by its file name (`of`), gives the contract prefix (`package::npm::`,
  `package::pypi::`), reads the bytes (`read`), checks a range (`flag`) and holds the
  `workspace:` rule (`reaches`, npm only); `Providers` is keyed by ecosystem and name, so an
  npm and a PyPI package of one name never link, and `nearest` takes the ecosystem (a file's
  nearest `package.json` and nearest `pyproject.toml` are apart). A file belongs to the package
  of its `owner`: the nearest `package.json`, or the nearest `pyproject.toml` (a
  `requirements*.txt` names no package, so it bounds none; a `Cargo.toml` with no `[package]`, a
  virtual workspace root, bounds none either). `package/pypi.rs` reads a
  `pyproject.toml` (through `lang::python::pyproject::Pyproject`, the one parse the pack's
  source roots share: `[project] name`, else `[tool.poetry] name`, `version`, `[project]
  dependencies` as PEP 508 strings, `[tool.poetry.dependencies]` but `python`, the declared
  `roots` and the console `scripts`, `[project.scripts]` else `[tool.poetry.scripts]`, as
  `(module, function)`) and a `requirements*.txt` (line by line, comments, option lines, URLs and paths
  skipped), each name normalized as PEP 503 does (`normalize`) and each specifier kept as
  written, extras and markers dropped (`requirement`). A PyPI range is never evaluated:
  `reason` ends `, range unchecked`; a Poetry `git` or `url` source without a `path` is a direct
  reference, no entry, as `name @ url` is, and a requirements line ending in `\` continues on
  the next. A call from a `.py` file (`Ecosystem::of_source`) resolves through
  `pypi_resolutions`: the import names a module, so each distribution a PyPI manifest of a
  directory holding the file depends on (`holding`: every `pyproject.toml` and
  `requirements*.txt` above it, the nearest first, whose entry gives the range) is a candidate when a provider exposes the import's top-level module under its roots
  (`python_roots`: the manifest's directory, then its declared roots or `src`;
  `Provided::module` finds `a/b/__init__.py` or `a/b.py`), `exact` with `How::ModuleName`
  (reason `; import module name`) when that module is the normalized name with `-` turned into
  `_`, `inferred` with `How::TopLevel` (`; import top-level module`) otherwise, and `ambiguous`
  for each when several distributions expose it (one consumer per distribution at the call
  site). The chain is followed from the module the specifier names (`import shared_lib` then
  `shared_lib.api.greet()`: `shared_lib`, then `api.greet`; `from shared_lib.api import greet`:
  `shared_lib.api`, then `greet`), and `Provided::member` takes a package's submodule when its
  `__init__.py` exports no such name. `surface` reads npm manifests alone: no shape crosses a
  PyPI package.
  Cargo (spec #30): `Ecosystem::Cargo` names `Cargo.toml`. `package/cargo.rs` reads one through
  the Rust pack's walker (`lang::rust::crates::entries`, also what the module trees use): `name`
  and `version` of `[package]`, `main` the library root as a path of the repository (`crates::lib_root`:
  `[lib] path`, else `src/lib.rs`, folded from the manifest's directory), `lib_name` the `[lib]
  name` when a `use` writes something other than the package name (`crates::lib_name`, also what
  the module trees use), and the entries of `[dependencies]`, `[dev-dependencies]` and
  `[build-dependencies]`, also under `[target.*]`, as `Entry` with `name` the real crate
  (`package =`), `alias` the key when renamed, `range` the version requirement and `via` where it
  comes from: `path <manifest>` (`path` folded lexically from the manifest's directory: a path of
  the repository, or one that starts with `../` when it leaves it), `git <identity>`
  (`repo::normalize_remote`) or none. An entry whose declared source cannot be read (an empty or
  mistyped `path`, `git` or `package`, an absolute `path`, both `path` and `git`) is dropped, not
  read as a version dependency. A `workspace = true` entry is kept as `inherits`,
  `[workspace.dependencies]` as `Summary::workspace`, `version.workspace = true` as
  `inherits_version` and `[workspace.package] version` as `workspace_version`; `cargo::inherit`,
  run by `index::read_manifests` over the whole list, fills each from the nearest manifest above
  it, so a refresh equals a cold export. The consumer site names the key
  (`<repository>::<path>::<field>::<key>`) and `Consumer::via` carries `Entry::via`, which
  `Ecosystem::reaches` checks against the provider: a `path` of the repository only the manifest
  at that path of the same repository, a `path` that starts with `../` only the manifest whose
  location, `Export::root` (set by `Engine::exports`, or `Repository::root` in `link`) joined with
  its path, folds to the same absolute location (lexical, nothing is read outside a repository,
  no symlink is followed; a path landing in no workspace repository links to nothing), `git` only
  the repository of that identity, a version every repository. The range is never evaluated
  (`range unchecked`). A `use` (`Packages::extract` stores the call when `Code::imported` names a
  crate head, `cargo::names_crate`: not `std`, `core` or `alloc`) resolves in `resolutions`
  through the file's nearest named `Cargo.toml`: the head, hyphens as underscores, must equal the
  key of a renamed dependency or, for one that is not renamed, the provider's library name
  (`cargo::lib_head`: `[lib] name`, else the package name), and `Provided::resolve` then follows
  the chain from the provider's library root through the `exported` rows of the Rust pack (`pub
  mod`, `pub use`; an item behind a private module is not reached), `exact` with `How::CrateName`
  (`; import crate name`) and no `ambiguous` fallback. A crate of the repository itself is
  reached through the module tree instead, so a path or version dependency inside the repository
  links at the manifest only.
