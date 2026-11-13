# HTTP extraction and linking

IaC declarations and handler placement: [API Gateway, SAM and Serverless](iac-http-routes-666.md).

Infrastructure routing and its shared inference explanation:
[Services, Compose and Ingress](http-infrastructure-665.md).

`http/ts/clients.rs`: `fetch` consumers (also a call to a parameter defaulting to
  `fetch`; only in a pack whose `globals` has it; each with `Found::shape` from `response_reads`, spec #23: over the frames
  `Code::expand` climbed, the field paths read from the `.json()` of the response in the
  function making the `fetch` and in each wrapper up to the site, a wrapper passing on the
  response, the body or a path below it when it returns that one thing from the function its
  caller reaches), Python client consumers (`http/python/clients.rs`, below), Next.js app-router
  and Express (`http/ts.rs`), FastAPI, Flask and Django (`http/python.rs`), Spring MVC (`http/java.rs`), Feign and Retrofit interface methods (`http/java/declarative.rs`, consumers at a `Site::Def`), axum
  (`http/rust/axum.rs`) and actix-web and Rocket (`http/rust.rs`) providers (Next.js and
  Express ones each with a `Found::shape` from `http::ts::provider_shape`, the one place that picks a route's code source: `zod::Zod::returned`
  first, else `literals::Literals::shape` (spec #23; both read the 2xx bodies `provider_shape`
  lists once with `Literals::responses`): one `Literals` per `extract` (with its `Types` and their
  `Zod`, which `response_reads` and `provider_shape` reach through it), holding the budgets of
  each file; Express reads a handler once for every prefix it is mounted under),
  `normalize_path`, and the link rules (host,
  `base_url_env`, unique provider). `locate` makes a URL whose leading part Meada could not read
  (a hole, an undecided read, a `Part::Limit`) with a rest appended an unresolved consumer that
  carries `Found::lead` (`Lead {limit, path}`, stored in `contracts.lead`), never a hostless
  contract (#801); a reader gives a client base it did not read as `http::client_base()`, and
  keeps a path from the root certain over an unknown origin (`rooted_over_unread`) where its
  library's join does. A provider that serves every method has the method `ANY`
  (`http::ANY::<path>`): `http::fits(provided, wanted)` matches it to a consumer of any method
  on its path, and its link's reason ends with `; any method`; `route_fits` (HTTP's `candidate`) and a `trace` start
  (`POST /health`, HTTP's `starts`) take it the same way. `http::drift(links, exports)`, HTTP's `after_links` pass, sets `Link::drift` on every
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
  `(contract, reason)` and `http::serves(provider, contract)`, HTTP's `serves`, is the one check of any path, through `fits` (also
  reached by open items and decision replay through the seam's `serves`, so a gateway candidate carries the provider's own
  contract). That link is `inferred` with the form's reason (`gateway prefix <p>`, `gateway
  strip <s>` or both), read from the provider, not recomputed from the configuration; within
  one repository a code path match wins over gateway path matches. An app or router
  (Express `express()`, `Router()`, and the default import of `express-promise-router` under any name; FastAPI `FastAPI()`, `APIRouter()`; Flask `Flask()`,
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
  is the one provider-side macro read; each handler in it resolves exactly through `use`, or yields nothing (`Code::outside` with the path flag reads the outside name of a callee, a path call's head among types and modules, so a `fn rocket()` does not hide the crate in `rocket::build()`; a bare route attribute comes from the `#[macro_use] extern crate` of its crate's root; `Code::mentions` tells whether a file refers to a name, which, over the handler's crate (every crate when `Code::published` says its file may be imported), decides whether an unregistered handler is served at its own path), reached through `Code::origins` with `Follow::Values`: a route counts only
  on a receiver that traces back to its framework's constructor (named by `Code::outside`; Express
  also takes an unimported or local name), so `@app.get` on a Flask app yields nothing.
  The Python framework and Go router tables have static storage: identity comparisons for mounts
  and the standard mux use one allocation, including in optimized builds. Each
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
  `aiohttp`): a call is a consumer when `Code::outside` names it a module function of a
  library imported by the file (`requests.get`, `httpx.post`, also under an alias), or when it
  is `get`, `post`, ..., `request(method, url)` on a receiver that `Code::origins` (`Follow::Values`) traces back
  to a constructor of the library (`Session()`, `Client()`, `AsyncClient()`,
  `ClientSession()`): assigned in the scope, bound by `with`, a module name, a `self`
  attribute, or a parameter the callers pass one to, or a value a function of the repository
  returns. This is wider on purpose than the three binding forms spec #24 names (same scope,
  `with`, a `self` attribute), as the final review accepted: every way `Code::origins` follows
  a value counts (module constants, caller parameters, return values). A `get` on any other
  object is nothing. Rust clients (spec #30 #131) are the same code with two more `LIBRARIES`
  entries: `reqwest` (modules `reqwest` and `reqwest.blocking`; clients `Client::new`,
  `Client::builder()`, `ClientBuilder::new` and the `blocking` equivalents, a builder chain
  followed to its constructor by `fluent`, and every method on the way listed by `passes_on` (the table of `http/rust/frameworks.rs`, which the reader hands the question):
  any other method on an outside value ends the chain) and `ureq` (module functions; clients `agent()`,
  `Agent::new*`, `AgentBuilder::new`). The receiver of a chain (`Client::new().get(..)`,
  `builder.build()?.get(..)`) is `Call::receiver`, read by `Code::origins` with `Follow::Values`. The method
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
  reads the value. A pydantic-settings field is a `Settings.field` constant holding its `Part::Env` (`settings_consts`, #619). Java (#89): `System.getenv("X")` is a `Part::Env` in the pack, and a Spring
  `@Value("${key}")` field or constructor parameter holds what `jvm/spring.rs` reads of the
  module's `application.properties`, `application.yml` or `application.yaml` (the module of the
  main source root a Maven or Gradle build declares for the file, else of its `src/main/`)
  (`Code::expand_with`): a literal URL links by host, a placeholder no key fills is
  `Part::Env`, and `${X:default}` is `Part::EnvOr`, read both ways by `http::fetches` as twins
  (`via = env:X|host:<default host>` and `host:<default host>|default`), of which
  `Http::workspace_step` keeps the default's when a provider of the workspace serves that host,
  else the variable's. `Code::expand` follows a function whose URL
  or method depends on its parameters, so each of its call sites is a consumer (an argument
  it leaves out takes the parameter's default). In Express `use(first,
  ...)`, `first` has no path only when it is a router (a call, also one `Code::origins` reaches
  through arguments and return values) or a handler (a function or a definition); anything
  else is an unreadable prefix, so what it mounts is served under no prefix.
