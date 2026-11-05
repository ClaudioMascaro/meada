# HTTP routes and clients (JVM, Go, Rust, TS)

- `http.rs` (`is_test_file`, `drop_test_providers`, #620): a route is a test's by where its
  handler is defined, so a test registering a handler of the repository's own code
  (`mux.HandleFunc("GET /orders", realHandler)` in a `_test.go`) leaves it a provider; and only the
  conventions the ticket names are test files (`*_test.go`, `*.test.*`, `*.spec.*`, `test_*.py`,
  `*_test.py`, `tests/`, `__tests__/`, `src/test/`), not `*Test.java`, `*Tests.cs` or
  `conftest.py`, nor a singular `test/` or a `spec/` directory, `*.e2e-spec.ts` apart from `*.spec.*`,
  Swift's `Tests/` (the match is case-sensitive), Go `testdata/` or Gradle `src/integrationTest`.
  A fake defined in a non-test helper file stays a provider. Channel providers and every other
  kind are untouched. `test_routes = true` keeps every one.
- `http/java.rs` (spec #26, #86): a Spring MVC route is read from `Call::decorates` on a method
  of a `@RestController` or `@Controller` class, under the class's `@RequestMapping` prefix
  (nested classes do not inherit the outer prefix). An annotation or `RequestMethod.X` counts
  only when `Code::outside` says an import from Spring binds its name: single-type, static, or
  the one on-demand candidate in the kind's table (`Outside::candidates`: `...bind.annotation.*`
  alone or beside `java.util.*`). A type of the repository with the same name (a class, an
  interface or an annotation type, which the Java pack indexes as an interface; a nested,
  same-package or repository-wildcard `GetMapping` or `RequestMethod`), a single-type import of
  another package and two candidates in the table yield no route; the kind reads the enum
  member off the written `Value::Ref`, with no table in the pack. A path folds constants and
  `+` through `Code::value`; one that does not fold, a `value` with a `path`, a class-level
  `method`, a `method` with no readable element, or a pattern with a `/` inside braces
  (`{id:a/b}`) serves nothing. An unreadable element beside readable ones of a `method` or a
  path array is dropped: the others still serve. Interface-declared mappings, composed annotations
  and `server.servlet.context-path` are not read.
  Kotlin (spec #27, #97): the Spring reader reads Kotlin files too, through `jvm_files` (`.java`,
  `.kt`, `.kts`, in the shared `Code`), with the same annotations and rules: the Kotlin pack
  writes `["/a", "/b"]` as a `Value::List` and `method = [RequestMethod.X]` as a keyword list, and
  a bare constant of a class's companion folds (`Resolver::inherited_field` reads the companion
  after the family, unless its own bases are unknown: a ceiling, since a same-named name outside
  then wins over the companion's inherited member). Several unnamed paths (`@GetMapping("/a",
  "/b")`) fill the `value` array, and `arrayOf(..)` counts as `[..]` in a path or a `method`
  when `known` says the name is `kotlin.arrayOf`. The pack writes a star import as an `inside`
  glob (it sees `internal` members), so `Resolver::on_demand` takes the outside source of an
  `inside` glob too (never the repository's names), and an outside star import
  (`...bind.annotation.*`) gives `known` its candidate, as Java's does.
  JAX-RS and Feign keep `java_files`; the `RestTemplate`, `WebClient`, OkHttp and Retrofit readers
  take `jvm_files` (spec #27, #101): Kotlin writes the same clients without `new`
  (`RestTemplate()`, `Request.Builder()`, a `Call::receiver` of `Retrofit` and no `Call::on`),
  so the `uri` and `build` steps of `clients.rs` match a chain step spelled
  `(call_expression).name`, `constructs` takes a `RestTemplate(..)` call for the written type, and
  `made` falls back, after typed and origin lookups, to `Code::local` (Kotlin's
  `Facts::bound` (Kotlin's `val`, a Java local the file never reassigns), shared with the Ktor
  reader and indexed by scope and name in `ByName`, so a
  lookup costs its scope depth) up to `jvm::names::LOCAL_DEPTH`. The value evaluation reads the
  same local (`Code::eval`, a `Binding::Hidden` or `Binding::Local` with a bound): a URL or a
  topic written through a local `val` of a function folds where the `val` is written, an
  environment read through `?:` included, as TypeScript folds a function's `const`. The pack binds every local
  `val` with an initializer, whatever its class (a factory call `WebClient.create(..)` or a
  `.build()` has none), and drops the bound of one whose name a loop variable or a `catch`
  parameter also declares in the scope; `local` then does not take the `val`'s own entry in
  `Facts::locals` for another binding (a typed one still has its receiver). A client held by such a
  local gets its base from `folded_local` (`http/java.rs`): the call it is bound to, folded by
  `Expand::Chain` where it is made, replaces the evaluated `Ref` that `sends` (`Client::Web`) and
  `declarative::base` would read as an unknown base. The Kotlin pack reads `X::class.java` as
  `Value::Member(X, "class")`, the Java class literal, and gives a call's one type argument
  (`create<Api>()`) as the same member under the keyword `lang::kotlin::TYPE_ARGUMENT`, which no
  name matches; `retrofit_clients` takes it for `create` only when `retrofit2.create` is what
  `known` says the name is. In `declarative.rs`, `builds`/`base` take the Kotlin constructor
  `Retrofit.Builder` as the chain start, and the bases of a `.kt` file expand with Spring's
  `@Value` inject (Java Retrofit bases stay as they were).
  JAX-RS (#87, `jax_rs` in the same file): a method with a verb annotation (`GET` ... `OPTIONS`)
  of a class with `@Path`, from `javax.ws.rs` or `jakarta.ws.rs`, joined to the method's own
  `@Path`; both packages through the same `known`, so `import javax.ws.rs.*` beside
  `jakarta.ws.rs.*` is two candidates and serves nothing. A `@Path`-only method and
  `@ApplicationPath` are not routes, and neither is a method of an interface (`Facts::interfaces`):
  a `@Path` interface is a MicroProfile or Quarkus client. A controller or resource class whose
  family has an outside base (`extends com.corp.Base`, top-level or nested) still gives its
  providers (#354): the callee of a decorator call (`Call::decorates`) is read as
  `Reading::Decorator` (`Code::outside_decorator`, `names::annotation`), which skips the inherited
  field pass, so an opaque ancestor hides no import from an annotation, and single-type and
  on-demand imports agree. The one miss, accepted and not detected, is a member type of the
  outside base named like the annotation (JLS 6.4.1 would put it before the import); the same
  reading serves the declarative clients and the channel listeners, in Java and Kotlin. A fully
  qualified annotation without an import (`@javax.ws.rs.GET`) is read too, as `names::known` and
  `annotation` take a dotted name whose head nothing binds as its own spelling (`written_in_full`,
  `in_full`), the repository winning: an import, or a definition of the repository named by the head
  or by the full name, keeps its reading. Only the annotations an implementation inherits from an
  interface are not read.
- `http/kotlin.rs` (spec #27, #98): Ktor routes, through the same `Mounts` and `prefixes` as
  the other routers, whose nodes are definitions (file, qualified name): the lambda of each
  `routing { }` call (a root), the lambda of each `route(prefix) { }` call (mounted under its
  prefix on the routing scope around the call) and each extension function on Ktor's `Route` or
  `Routing` (mounted with no segment on each routing scope that calls it through the implicit
  receiver, so one called under two prefixes serves under both; one nothing calls is served
  nowhere). The routing scope of a call is the nearest such node around it through lambdas
  passed as arguments (`Kind::Inline` scopes inside a call of the scope around them, by the
  calls' and definitions' ranges), stopping at a route's handler and at a local function's
  body. `get`, `post`, `put`, `patch`, `delete`,
  `head` and `options` with a path or none (the block's path), and `route(path, HttpMethod.X)`,
  register a route whose handler is the trailing lambda (`handler_site`); the path and the method
  are read positionally or by name (`path =`, `method =`). The extension called
  is the nearest one in scope (`Code::extensions`, Kotlin's lookup order: around the call, an
  explicit import, the package, a star import, after the locals and parameters that hide the
  name) whose receiver is Ktor's `Route`, or `Routing` in a scope whose receiver is one:
  the routing block tells the implicit receiver, which the resolver's own answer for the call
  (a name match inside a lambda) does not know. A Ktor name counts only when `Code::outside`
  says an import from `io.ktor.server.routing` (or Ktor 1's `io.ktor.routing`) binds it, and
  `HttpMethod` from `io.ktor.http`; a name a star import of `io.ktor.server.resources` may bind
  too is no route, so type-safe routing is not read. Ktor parameters (`{id}`, `{id?}`, `{...}`)
  fold as actix-web's (`actix_path`).
- `http/kotlin/client.rs` (spec #27, #99): Ktor client calls (`get`, `post`, `put`, `patch`,
  `delete`, `head`, `options` and `request` of `io.ktor.client.request`, counted by the import
  `Code::outside` says binds the verb) on a receiver that is an `io.ktor.client.HttpClient`: one
  `Code::origins` (`Follow::Values`) traces to a call to it (counted by the import of its
  callee as `names::spellings` reads it, so `import io.ktor.client.*` counts), a local the
  pack recorded (`Facts::bound`: a `val x = Callee(..)` with no written type, and the parameter
  of a `let`, `also` or `use` lambda, whose value is the receiver; `Code::local` takes the
  nearest binding of the name and none when another binding of it shares its scope, so a
  parameter, a loop variable, a `var`, a local named like a property of its class, of an enclosing class or of the file, or one declared
  twice hides it) followed through `origins` again, or one whose written type is it
  (`names::typed`); a map, another library's client and a name a local hides are none. The
  call is read whole (`Expand::Chain`, which also gives the reader the call's keyword arguments),
  so an enum member a wrapper passes keeps the evidence of the caller's imports. The URL (first
  argument, or `urlString`) goes through `Code::expand`, as the Go clients do, so a wrapper is
  read at each call site of the function that determines the URL or the method, through any
  number of levels, with cycle detection; a call with no URL argument (`get { url { .. } }`) is
  an unresolved consumer. The base of a client made by a call is the `url(..)` of the
  `defaultRequest { }` in the configuration lambda of that call (Ktor 2 and 3
  `io.ktor.client.plugins.defaultRequest`, Ktor 1 `io.ktor.client.features.defaultRequest`),
  folded in its scope: a hole when it is set more than once or not by a string, none for a client
  known by its type alone. `request` sends the method its block assigns: the pack records any
  property assignment `p = v` or `this.p = v` written as a statement of a lambda argument as the
  `p=` key of that call's `Call::kwargs` (`kotlin::assigned`, a key no Kotlin name matches and the
  pack names no property; `given` in `http/kotlin.rs` and in `http/kotlin/client.rs` ignores keys
  ending in `=`), `Unknown` where the statement may not run (inside an `if`, `when`, `try` or
  loop) and, for the call that owns each lambda around a nested assignment (`headers { method = X
  }`), because the inner lambda may not run, and when the block assigns it twice. The Ktor
  client reads the `method=` key (`client.rs`). `GET` when it assigns none and the call is given no other argument, a `Ref`
  while it depends on a parameter of a wrapper, unknown for anything else (a block passed as a
  value, `HttpMethod.parse(..)`, a method that is no `HttpMethod` member of Ktor).
  The Kotlin pack folds a string literal's templates into parts (`$name`, `${name}` as names,
  any other `${expr}` as a hole); the grammar reads `$name` as text, so the pack finds the name
  in the text. A literal with an escape other than `\$` is unknown; `\$` is a `$` that starts no
  template, so Spring's `@Value("\${key}")` is the text `${key}` (#100).
  Base URLs (spec #27, #100): the client reader expands with `Code::expand_with` and
  `jvm::spring::inject` (as `http/java/clients.rs`, `Configs` read the first time an annotation
  is injected). The Kotlin pack records `Facts::annotated` for a property (a field of its class,
  `static` at the top level or in an object or a companion, where Spring injects nothing), a
  constructor parameter (`Marked::Param` of the class, and the `Marked::Field` of the property
  a `val` or `var` declares, whose `Const` is dropped when annotated, as a `lateinit var` has
  none, so the annotation is what it holds) and a function's parameter, by use-site target: none,
  `field`, `set` and `setparam` for a property, none and `param` for a parameter; `get`,
  `property` and the others inject nothing. Every assignment to a name that can be a
  property (`base = ..` of a name no local or parameter declared before it, `this.base += ..`, `base++`, `o.base = ..`)
  is a `.name` constant, or `.name@class` when the pack knows the class, as Java's, which
  `Code::field_written_in` reads; an assignment to a local or a parameter is not one, and a classless write
  matches the name in every class: a field some code of its class assigns holds no injected value. `value()` reads
  `System.getenv("X")`, `java.lang.System.getenv("X")` and `System.getenv()["X"]` as `Part::Env`,
  through `!!`, parentheses, `?:` fallbacks and the string methods of `STRING_METHODS`
  (`trimEnd`, `removeSuffix`, ...), unless the file binds `System` to another type
  (`SYSTEM_BOUND`, as Java's); each node is read once, a string method passing on the
  `Part::Env` its receiver already evaluated to. A fallback that reads the environment too (a
  `System.getenv` call, or a property of the file whose declaration holds one, `ENV_NAMES`) makes
  the term a hole: exactly one read. Adjacent literal parts of a `+` join, so `"\$" + "{key}"`
  is a placeholder. The variable maps through `base_url_env` as in the trunk. A function-local
  `val` is no constant in the Kotlin pack, so the read must stand in a property or the URL
  expression.
- `http/java/clients.rs` (spec #26, #88): a RestTemplate or a WebClient counts by the type the
  source writes for the receiver (`Follow::Declarations`), or, for a WebClient, by the
  `WebClient.create` or `WebClient.builder()` chain it traces to (`Follow::Values`); a name
  imported on demand counts through its one candidate in the kind's table (`Outside::candidates`,
  now also for a head only the file's outside names bind). A WebClient's base is read off the
  folded chain (`Expand::Chain`), through constants, `final` fields and a wrapper parameter's
  arguments; a client held in a local has an unknown base, so the consumer links by path. The
  Java pack keeps a `final` instance field's one value (its initializer, or the single `this.f =
  v` or `f = v` of all its constructors; two give none) as a `Const` of its class, outside
  `statics`, a constructor's value read in that constructor (`Const::scope`): its parameters
  are what Spring injects (#89), else unknown. A builder
  (`HttpRequest.newBuilder()`, `new Request.Builder()`) is read only as one chain ending in
  `build()`; one held in a variable between steps is no consumer, and a chain folded short of
  its start names no method. `String.format` puts each `%s` or `%d` argument in place (a
  parameter keeps the wrapper climbing, anything else is a hole) and reads `%%`; any other
  conversion leaves the URL unread. A `HttpMethod.X` names a method only when one spelling
  (`Outside`) is Spring's.
  Bases from configuration (#89): the reader expands with `jvm::spring::inject`, so a field or a
  constructor parameter with Spring's `@Value("${key}")` (bound by an import, as for the
  annotations of #86) holds `key` from the `application.properties`, `application.yml` or
  `application.yaml` of its module's `src/main/resources`, in that order of precedence (a file
  past `lang::MAX_BYTES` is not read, nor a `.properties` past its first `#---`). A source's
  module is that of the deepest main source root a Maven or Gradle build declares for it
  (`Indexed::manifests`, a Gradle source set's `srcDirs` included), else the one whose
  `<module>/src/main/` it is under. The pack records every annotated field with whether it is
  `static`, and `spring::inject` gives a `static` one nothing; a field some file assigns, and
  one a parameter, a local or a same-name binding hides, hold nothing too. A placeholder no key fills is the variable it
  names (`Part::Env`), and `${X:default}` keeps both (`Part::EnvOr`): `http::fetches` reads
  such a URL through `X` and, when the default names a host, through that host too, the twins
  marked in their `via` (`env:X|host:<host>` and `host:<host>|default`). `Http::workspace_step`
  (`keep_applicable`), after every `declare` filled `Provider::hosts`, keeps the one that
  applies, its mark dropped: the default's when a provider of the workspace serves its host,
  else the variable's. Links, `context`, `trace` and the open items see one consumer per call.
  The Java pack reads `System.getenv("X")` (or `java.lang.System.getenv`) as `Part::Env`,
  unless the file imports or may declare another `System` (`binds_system`, a word after
  `class` in a comment counts; `import java.lang.System;` does not). Profiles, relaxed binding,
  YAML lists and a SpEL `#{...}` value are not read. The configurations are read once per
  extraction, the first time an annotation is injected.
- `http/java/declarative.rs` (spec #26, #90): a Feign or Retrofit interface method is a consumer
  sited at the method (`Site::Def`, built by `http::consumer_at`), not at a call: its callers
  reach it as any call to a definition. Feign: `@FeignClient` from
  `org.springframework.cloud.openfeign` (bound as the Spring annotations of #86, through `known`)
  on an interface; `url` (empty is none; without a scheme it gets `http://`, as Spring Cloud does) and `path` resolve with `jvm::spring::resolved`
  over the module's configuration, the methods' mappings are `request_methods` and `paths` of
  `http/java.rs`, a `@RequestMapping` with no `method` meaning GET, as Feign sends it. Without
  `url` the consumer's `via` is `client:Feign client <name>`: `http::CLIENT_VIA` is the prefix of
  a client that names itself, the rest the words the reader writes, and `Http::link` words the
  unresolved host as `host unresolved (<words>)`, so the shared code names no library; a name with
  `|` is refused, it would collide with the twin marks. A `@RequestMapping` on the interface,
  `@RequestLine` and other contracts give nothing. Retrofit: the verbs of `retrofit2.http`
  (their names collide with JAX-RS, so `known` accepts both spellings and the result must be
  Retrofit's); the base is the folded `Retrofit.Builder().baseUrl(..)` chain of each
  `retrofit.create(Api.class)` (`Expand::Chain`, climbing wrapper callers while the base holds a
  `Part::Ref`), the receiver proved Retrofit by its type or by its `new Retrofit.Builder()`
  chain; one consumer per creation, none gives an unknown base. Its annotations name the path by
  `value` alone, and a route with a brace open past its segment is not read (`balanced`, as for
  Spring, JAX-RS and Feign). A route from `/` replaces the
  base's path (read over the base's leading text parts, so `HOST + "/v1/"` counts), and an empty
  route is Retrofit's dynamic `@Url`: a consumer with no contract. A consumer at a method is an
  open item like a call's (`items::list`): its location is the method, its expression the method
  on one line, and its enclosing symbol the method itself, so `meada decide` sends the method's
  lines as the minimal excerpt (ADR 0006). The Java pack reads `Api.class` as `Value::Member("Api", "class")`.
  `ContractKind::calls_from_definition` (true for HTTP) tells `trace` that a consumer at a
  symbol is a call to follow, unlike a channel handler, where the link ends.
- `http/go.rs` (spec #25): a route is read only with a literal path and a literal prefix; a
  group prefix or a pattern Meada cannot read serves nothing. `net/http` reads `{name}`,
  `{name...}` and `{$}` alone: a `:name` segment is a literal no contract can spell, so its route
  has no provider, and `*` stays literal; gin and echo read `:name` and `*rest` as parameters.
  A `ServeMux` is mounted only as the handler of a `Handle(pattern, mux)` (or a function
  returning it) on a router Meada reads; it keeps its own paths, served for the pattern's
  subtree (a trailing `/` or `{name...}`, else the exact path) and method alone, so a route
  outside the pattern, or of another method, has no provider. A mux held by any other argument of
  a route call (`http.StripPrefix`, `echo.WrapHandler`, `gin.WrapH`, any middleware), or given
  to a router held in a field, a map or a slice, is an unread mount: served nowhere. A mux
  nested deeper than `PREFIX_DEPTH` or through more mounts than `PREFIX_WORK` is served nowhere
  too. `http.StripPrefix` and other middleware that rewrites paths are not modelled (spec #25).
  Middleware (`Use`, extra handlers) has no effect on a gin or echo path. A router held in a
  struct field, a map or a slice is not followed (no marker).
  chi: `*` is a parameter segment, `:name` a literal no contract can spell (no provider);
  `Mount(prefix, router)` follows the router like a group, any router inside a call of the
  handler argument (a middleware), and a `ServeMux` given to `Mount`, are unread mounts (chi's
  `Mount` leaves the request path alone, so the mux would see its full path under the mount's
  prefix: combining those chains with the router's prefixes is not modelled); `With(..)` is not
  a group. A `Route` or `Group` function binds its first parameter to the group of that call,
  and the group is followed from there through exact calls (`users(r)`) and names bound once
  (`sub := r`), at most `PREFIX_DEPTH` hops and `PREFIX_WORK` parameters (past it the group is
  served nowhere); a call found only by a name passes nothing on, and a parameter
  rebound anywhere in the function hides it in the whole function. gorilla: `.Methods(..)`
  restricts the call it is chained on through `Name`, `Schemes`, `Headers`, `Queries` and
  `Host`; arguments Meada cannot read, no argument and a second `Methods` serve nothing;
  `Path(..).HandlerFunc`, `NewRoute`, `Host` subrouters and `StrictSlash(..)` chains are not
  read (a router made by a call that is not `NewRouter` has no provider).
- `http/go/clients.rs` (spec #25, #79): `CLIENT_DEPTH` (64): a client or a request is followed
  back through that many calls, names and struct fields (`New().SetX().R().SetY()`, `b := a`,
  `c.rc`); past it the base is unknown and the request is no consumer (the chain is no longer
  known to be resty's). A request on a client only written as a type (a parameter, or a field
  no composite literal sets) is a consumer with no host, and so is one on a parameter some
  caller passes an untraced client to (the base is unknown). A field's literals are those of
  its type: a literal of an alias or a defined type (`type B = T`, `type B T`) written by
  position, and a conversion from another struct type (`T(other)`), are not seen as setting
  the fields of `T` (`Code::field_values`), and a literal nested deeper than `VALUE_DEPTH`
  inside literals written without their type sets the fields of a type not read. Only the
  libraries of the table (`net/http`, resty v2 and v3, `resty.dev`) are
  clients; a client assigned more than once, one held in a map, a slice or a channel, or an
  interface, is untraced and its requests are no consumer. A resty verb called on a struct field
  with no `R()` before it (`s.repo.Get(..)`, #621) is a request only when a literal gives the
  field a traced request or a value written as `resty.Request`; a field of another type, or one
  written through a selector, is no request (the field's declared type is not read), so a
  `*resty.Request` field set only that way, or from a helper outside the repository
  (`&T{req: sdk.NewReq()}`), loses its consumer; a field met again while it is followed still
  gives an unknown request. A `fmt.Sprintf` format with a verb
  other than `%s`, `%d` and `%v`, a count of verbs and values that differ, a `var` assigned
  anywhere but its declaration (or written through a selector), fold to nothing: the consumer is
  unresolved (no marker). A verb with flags, width or precision is a hole, and so is `%d` of
  anything but an integer literal, and `%s` of an integer literal. `values::Std` folds
  `fmt`, `net/http`, `os`, `strings` and `aws` only where nothing else in the file spells the
  name they are imported by (`hidden`: a parameter, a local or a function of that name in any
  function of the file leaves every call of the package in the file unfolded, whatever its
  scope).
- `http/rust.rs` (spec #30): `ITEM_WORK` (16384): the values the expansion of constants and
  returned lists adds, over the whole repository, to the handlers a call names; a list past it
  is read no further, and a handler it left out is taken as mentioned. `macro_use`: the crate
  of a bare route attribute is read from the `#[macro_use] extern crate x;` of its crate's root
  (the `Tree`'s `crate`, wherever `[lib] path` puts it), and it needs exactly one such crate
  (actix-web or Rocket). A handler is served at its own path only when no file of its crate
  mentions its name (`mentions.rs` above), or of any crate when its file is one another crate
  may import (`Code::published`, a library's `pub mod` files); any other mention Meada does not
  read as a registration (`services![..]`, `vec![..]`, `concat()`, `cfg_if!`, an alias, a use
  before a `let` of the name) means it may be registered where Meada cannot tell, and then it is
  served nowhere. A file two crates reach belongs to the first (see above), so a mention in the
  second, of a handler not published, is not counted (no marker). A service configuration is followed into the functions of the
  repository it is passed to (`f(cfg)`, answered exactly), not through methods or closures.
- `http/rust/axum.rs`: `METHOD_CHAIN` (64): an axum method router chain (`get(a).post(b)`) is read
  to that many calls, a longer one gives its route no provider. `FLUENT_CHAIN` (1024,
  `resolve.rs`): a builder chain (`Router::new().route(..)`, an actix-web `ServiceConfig`'s
  `cfg.service(a).route(..)`) is read to that many calls down to what starts it (`Code::chain`),
  and a call past it has no origin, so its route is no provider (the walk is memoized per call,
  linear in the chain). `PREFIX_DEPTH` (64) and `PREFIX_WORK` (4096):
  `prefixes`, for every kind that mounts (Express, FastAPI, Flask, axum, actix-web, Rocket, Ktor,
  ASP.NET Core minimal APIs), reads a router
  nested deeper than the first, or through more mounts than the second, as served nowhere
  rather than under part of its prefixes. A name bound again in its scope (`let mut`,
  shadowing) or assigned again holds no known value, and the routers its `let` holds
  (`Facts::rebound`) are served nowhere; `on(MethodFilter, h)` and a method router held in a
  variable are not followed. A router nothing mounts is served at its root. A router
  given to a call Meada does not read (`route_service`, `fallback_service`, an outside
  function wrapping it, a method only a name match found) is mounted nowhere, so it is not
  served at the root; so is one whose `nest` prefix cannot be read, or that is mounted on a
  router Meada cannot read. A router handed to a call that is neither a mount nor a wrapper
  of one in its arguments (a router put in a struct, a closure capture) is not told from one
  nothing mounts. The axum reading of those unread values (`axum_unread`) walks each value
  once per file (its `Unread` state keeps the values walked across the file's starts), and asks
  `Code::origins` only of a call whose builder chain (`Code::chain`) goes through methods a
  router hands back, and of a name not written (`Code::written`) as text, a path, a number, a
  boolean or a character (`is_plain`, by the last path segment) (#431).
- `http/kotlin.rs` (spec #27, #98): a scope holding a `route` call whose lambda is not read (one
  nested past the Kotlin pack's `DEFINITION_DEPTH`, or a block passed as a function reference,
  `route(path, Route::orders)`) registers nothing: its routes and routing extension calls, the
  block's siblings included, are served nowhere, never under part of a prefix. The implicit
  receiver of a routing block is taken as Ktor's `Route` (`Routing` in `routing { }`), kept
  through any lambda argument inside the block (`authenticate { }`) but a handler's or a local
  function's: a member of `Route` named like an extension of the repository, and an extension of
  a supertype (`Any`, a type parameter) nearer in scope, are not weighed, and an extension on
  `Routing` called in a `route` block (which Kotlin runs on the `Routing` around it) mounts
  nothing. A local or a parameter of the name hides an extension (`Code::extensions`), and a
  local function of the name mounts nothing. Routes nested in `route(path, HttpMethod.X) { }`
  (its lambda is the handler), `method(HttpMethod.X) { }`, `install(Routing)`, a lambda passed
  by name (`body = { }`), any named argument other than `path` and `method`, and a path with a
  string template that does not fold are not read. A Ktor verb inside a lambda (`get`, `put`) is a call the
  resolver reads as a name match, as any call there is, so it may match a repository method of
  the same name: that method then has a caller and loses its export door.
- `http/kotlin/client.rs` (spec #27, #99): `url { path(..) }`, `url(..)` and `takeFrom` inside a
  request block, `HttpRequestBuilder` values, `submitForm`, the `prepare*` forms, an aliased
  import (`import .. as k`) and a client the call reaches through `with(client) { get(..) }`
  are not read. A `defaultRequest` base given by a parameter of a client factory is a hole. A
  local client is followed only when its `val` is written with the constructor call (or a
  `let`, `also` or `use` receiver): `val b = a` and a `var` are not, and a local named like a
  property of its class, of an enclosing class or of its file is typed unknown by the pack, so it is not either. A nested lambda's
  `method = X` makes the outer request's method unknown even when the inner lambda always runs
  (`headers { }`).
- Kotlin base URLs (spec #27, #100): `STRING_METHODS` also lets `replace`, `lowercase` and the
  others of the trunk packs' lists pass an environment read through, beyond `trimEnd` and
  `removeSuffix`. A fallback that reads the environment through a name of another file, a
  function or a local is not seen (`ENV_NAMES` is the file's properties). An environment read
  kept in a local `val` (`val raw = System.getenv(..) ?: ..; raw.trimEnd('/')`) or in a
  function stays open: the consumer is open, never a wrong link. Parity with Java, not
  regressions: a constructor parameter without `val` used in an initializer stays open; a
  call that constructs the class in the source (`ExplicitClient("http://localhost:1")`) does not
  stop injection into the constructor's property; `@Value` on a plain function's parameter is
  injected as if the function were a bean's. A secondary constructor's `@Value` parameters are
  not read, and a `@set:Value` property is taken as injected into its field.
- Kotlin HTTP consumers (spec #27, #101): a named argument on a nested builder step
  (`.url(url = ..)`) is dropped by `Invocation`, so the consumer is open, never a wrong link; an
  OkHttp `Request.Builder` held by a local `val` is not followed (the chain starts at the local's
  name, as in Java); a local that a wrapper's callers fold to several bases has an unknown one;
  Java Retrofit bases from `@Value` stay unread; Feign remains Java-only.
- `http/ts.rs`: `router.route(path)` chains and `require` are not followed. A route path or
  prefix that depends on a parameter is not read through callers (it yields no route;
  `http.rs` `path_literal`, for every framework). `http/ts/backstage.rs` (#615): a backend
  plugin mounts its routers under `/api/<pluginId>` only when `createBackendPlugin` is imported
  from a `@backstage/` package and writes `pluginId` as a string literal or as a `const` of the
  same file bound to one; any other `pluginId` (an environment read, an import, a parameter,
  a `const` bound to another `const`)
  mounts nothing, so its routers stay at the root, and `meada status` names the plugin
  (`Msg::BackstageUnmounted`). Only
  what `register` itself gives `<dep>.use(...)` is read, `<dep>` a `deps` key of a `registerInit`
  written in `register` and bound to `coreServices.httpRouter`: a helper `register` calls and a
  renamed destructured `httpRouter` (`{ httpRouter: r }`) are not followed, and such a plugin,
  like one whose `register` is not a function written in place, is neither mounted nor named by
  `meada status`. On the consumer side only
  a URL whose first part is `<x>.getBaseUrl('<literal>')`, held by a constant or written in
  place as the first call of the `fetch`, folds to `/api/<literal>`, and only when the receiver's
  last name holds `discovery` (`discovery`, `this.discovery`, `discoveryApi`; by name, not by
  type): another receiver, a base passed as a parameter and a non-literal id leave the base
  unknown. A
  hostless call names a plugin in its reason only for a route that plugin's mount serves in the
  consumer's own repository (`backstage::name`); in another repository it links as any hostless
  URL does. `http/python.rs`: a FastAPI
  mount whose child cannot be read hides every other app of its framework that nothing else
  mounts. Django: one `include()` whose argument cannot be read leaves no root URLconf; the
  walk reads at most `URLCONF_BUDGET` (2^15) entries over every root and `include()` (a
  module included under several prefixes is read once for each), so the entries past it yield
  no provider, and follows at most `INCLUDE_DEPTH` (64) `include()` calls, each of the next
  module, so the routes of a module included deeper yield none; what
  an extension of `urlpatterns` adds other than a list literal is left out (marked in
  `lang/python.rs`), and a single `urlpatterns = [...] + other` (no list literal as a whole)
  yields no entry; `django.conf.urls.url`, a view wrapped in a call in the URLconf
  (`require_POST(view)`), a handler method a class-based view inherits and `as_view()` on a
  class outside the repository yield nothing; a `re_path()` pattern is read as written, so a
  non-raw string with doubled backslashes yields nothing. Python clients: a receiver that is the
  result of a call (`Session().get(...)`, `make().get(...)`) or an attribute set from anything
  but a call (`self.http = http`, marked in `lang/python.rs`) is not read; a parameter given a
  client takes the base of every caller that passes one, and several different bases make it a
  hole.
- `http/ts/clients.rs` (`joined`, #686): a first argument `new URL(reference, base)` of the global
  `URL` (`Value::New` of `GLOBAL_URL`, so a file's own `URL` is left alone) is replaced by the
  join of the two before the `fetch` is read, in the argument, through a constant and in a client
  wrapper's call sites. The join is `http/join.rs`'s RFC 3986 resolver, which the WHATWG URL
  Standard's constructor agrees with for the references it reads: a literal base with its scheme
  and authority visible, and a reference that is absolute, `//host/x`, `/x` or relative (the
  base's directory ends at its last `/`, so `orders` over `https://h/api` is `/orders`). Over a
  base that is an environment read or a hole as its whole origin (`${API_URL}`, or followed by a
  path) only an absolute-path reference is read, `opaque_lead`: `/orders` keeps the lead
  (`http::GET::/orders`, `via env:API_URL`). Unknown, so unresolved: a relative reference or a
  `//host/x` over an opaque base, a reference that starts with a read or a hole, a query or a
  fragment alone (they keep a path the join does not place, even over a literal base), a base
  with its authority half opaque (`https://${H}` is read; `${H}:8080` is not), a `..` that would
  remove a segment holding a part that is not text, and a base or reference that is itself a
  `new URL`. A parameter on either side is resolved at the call sites, where the pieces are
  joined again. `.href`, `toString()` and `String(u)` of a two-argument `new URL` are not read.
- Backstage proxy endpoints (#616, `http/ts/config.rs`): a `fetch` URL whose leading literal text
  is `/api/proxy/<ep>` (also what `getBaseUrl('proxy')` folds to), `<ep>` an endpoint of
  `proxy.endpoints` in the `app-config*.yaml` files (the lookup of the configuration reads
  above), the longest matching endpoint, followed by `/`, `?`, `#` or the end of the URL. With
  no `proxy.endpoints.*` key in the flattened set of every file the endpoints are the keys of
  `proxy` that start with `/` (Backstage's legacy shape, #690: `endpoints` wins when any file
  has it, so a legacy key beside it is no endpoint); a string value is `{ target }` in either
  shape (`config::expand`, before the flattening), and the options another file gives the same
  endpoint apply to it. A `|` in an endpoint name is not read. An endpoint whose name has a `.`
  and a key named `target` below it are told apart only by the `.target` suffix, so a string
  endpoint named `/a.target` reads as the endpoint `/a`. A `proxy.endpoints` behind an include
  that is not followed is no key (an empty `endpoints` object is one, and hides the legacy
  keys), so the legacy keys beside it are read as endpoints (`meada status` names the include). A target that is exactly `${VAR}` is the variable, one that is
  a literal `http(s)://` URL is the host (a trailing `/` dropped, its path kept); any other
  target (`${VAR}/v2`, `${VAR:-default}`, a URL with a placeholder) leaves the URL as written, with a
  `client:` `via` the link's reason shows, and `meada status` names the endpoint and its calls.
  A `pathRewrite` is followed only when it is the one rule that strips the endpoint
  (`'^/api/proxy/<ep>': ''`, or `'^/api/proxy/<ep>/': '/'`, which strips only a path that starts
  with `/`) and the endpoint is plain text in a regex (letters, digits, `/`, `-`, `_`); any other
  (another replacement, several rules, a regex character in the endpoint) makes the target
  unread, so the call stays unlinked and `meada status` names it, as do `ignorePath`,
  `prependPath: false` and an endpoint key two `app-config*.yaml` files give different values. A
  rule with a null replacement, and a `pathRewrite: {}` or `null`, are not in the flattened keys
  and read as no rule (Backstage's default, the strip, is assumed). The other options (`allowedMethods`,
  headers) are not read. Only a consumer in the repository holding the `app-config` is rewritten
  (#616).
- Backstage configuration reads (#617, `CONFIG_READS` in `lang/ts.rs`, read by `http/ts/config.rs`): `config.getString('a.b')`
  and `getOptionalString('a.b')` with a literal key are read from the `app-config*.yaml` files of
  the repository, `app-config.yaml` first and the first file holding the key winning, whatever
  their directory: profiles are not merged and a monorepo's apps share one lookup. A value that is
  not exactly `${VAR}` (a literal, `${VAR:-default}`, text around the placeholder), a key no file
  holds and a read passed in from another file's wrapper leave the base unresolved. The pack reads the call by its member name alone, so the receiver is
  not checked: a `getString` of another library with a key a config holds reads the same.
- Backstage `$include` and `$env` (#689, `http/ts/config/includes.rs`): an include nests at most
  `DEPTH` (8) deep, one root follows at most `INCLUDES` (64) includes (every file it reaches
  counted, refused ones past the path checks included) and grafts at most `GRAFTED_MAX`
  (`lang::MAX_BYTES`, 1 MiB) of included files, each include counting its file again; the include
  past a bound, a cycle (a file already on the way from the root), an absolute path, a path that
  leaves the repository, a `${` in it, an extension other than `.json`, `.yaml` and `.yml`
  (case-sensitive, as Backstage's), a symbolic link on the way, a file the walk does not see
  (absent, ignored or hidden, all `missing`), a file past `lang::MAX_BYTES` or that does not
  parse, a missing key path, a sibling key and a value that is not text are not followed: the
  subtree is null, so a read under it stays a hole, and `meada status` names the include with its
  reason. A root past `lang::MAX_BYTES` is dropped. Not read at all, and not named: an include
  inside a list (lists are not flattened), `$include` at the root of a document and as the value
  an include gives (Backstage applies neither), `$env` beside another key (the object is unread)
  and `$file` (its object stays as written, so no read reaches the file). The rows `meada status`
  reads (`Role::Unfollowed`) have no code site and sit in the TS and JavaScript group's first
  file, so a repository without TS or JavaScript files names none. Backstage roots no longer
  drop a document holding Spring's profile keys (`spring.profiles`), which only `read_yaml` checks.
  The cycle check compares files, not key paths: a partial that includes another key path of
  itself is a `cycle`. A grafted root is flattened under `spring::flatten`'s `YAML_KEY_BUDGET`
  whole, so grafts that push its joined keys past it drop the root's keys, inline ones included,
  with no status line. The includes refused before the count (absolute, outside, `${`, extension,
  sibling, not a path) are each named, as many as the root writes (linear in its bytes). The graft
  recurses, bounded by `DEPTH` + 1 documents of at most `MAX_NESTING` levels each. A partial whose
  name ends with `tsconfig.json` joins the TS pack's configs and is read as a tsconfig too. The
  index grafts every root on each freshness check (`packs_of`), and the extraction once more.
