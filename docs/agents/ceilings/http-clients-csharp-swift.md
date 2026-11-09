# HTTP routes and clients (C# and Swift)

- `http/swift/client.rs`: `CLIENT_DEPTH` (8) bounds the locals, properties and constants followed
  from a call's receiver to a client; past it the receiver is no client and the call is no
  consumer. The pack's unit tests and the hostile-URL integration test exceed the URL limits
  (a `+` chain, `appendingPathComponent` chain or nested `URL(string:` past `VALUE_DEPTH` is an
  unresolved consumer). Beyond the limit, what it leaves out (spec #29, #121): see its entry.
- `http/csharp.rs` (spec #28, #109): an ASP.NET Core controller route is read from `Call::decorates`
  on a method of a class that is a controller: `[ApiController]`, or a base that `Code::outside`
  says is `Microsoft.AspNetCore.Mvc.ControllerBase` or `Controller`. The pack ships syntax only,
  so the attribute names live here: `Route`, `ApiController`, `ActionName`, `NonAction` and `Http<Method>`, with or without
  the `Attribute` suffix, count when a `using` of `Microsoft.AspNetCore.Mvc` binds them (the one
  on-demand candidate in the kind's table, through `names::known`)
  or when written in full (`names::in_full`), and a class of the repository that the
  attribute's scope reaches (`Code::definition`, named `<Name>` or `<Name>Attribute`), or that
  has the full name written with `Attribute` (`Hiding::full`), hides the framework's. A decorator
  named `Route` that is not confirmed as MVC's leaves its class or method unread (never a route
  without the prefix it would add). The class `[Route]` templates are the prefixes; each method
  attribute with a template is a route of its method, a method `[Route]` is one of every method
  the action's method attributes name, or of `ANY` (`http::ANY::<path>`, whose link reason says
  "any method" in `http.rs`) without any, and a method attribute without template gives the
  prefix alone when the class has a `[Route]` and no attribute has a template: with no class
  `[Route]` it is conventional routing, which is not read (`[Route("")]` is the root). A
  template starting with `/` or `~/` replaces the prefix; `[controller]` and `[action]` are
  replaced on the joined path (`[action]` is the method name without a trailing `Async`, as
  ASP.NET Core does by default, or the literal of `[ActionName]`; a non-literal one serves
  nothing), `[[` and `]]` escape the brackets, any other token or a pattern with a `/` inside
  braces (`balanced`, shared with `http/java.rs`) serves nothing, and `?` of `{id?}` is dropped
  before the trunk's normalization, which reads it as a query. Case is kept, so `[controller]`
  on `OrdersController` is `/api/Orders` (ASP.NET Core matches case-insensitively; the trunk
  does not). A method serves nothing when `[NonAction]` marks it or `Facts::hidden` says it is
  not public. Ceilings: a template that does not fold to a literal serves nothing; a class with a base class of the repository (an interface is fine), a
  class that is the base of another one of the repository, and a class declared in parts
  (`Facts::partial` in several files) serve nothing, since the attributes they have or pass
  down are not followed or joined; `[Area]` and `Name=` route names are not applied;
  `SuppressAsyncSuffixInActionNames` turned off is not read; `[HttpGet("a")] [Route("b")]`
  serves `GET /b` (ASP.NET Core may serve it for every method), and a method attribute without
  template beside a templated one gives no route.
- `http/csharp/minimal.rs` (spec #28, #110): minimal API routes, through the same `Mounts` and
  `prefixes` as the other routers, whose nodes are the origins of a builder (`Code::origins`
  with `Follow::Values`, each with the calls the walk climbed through return values). A
  `MapGroup(prefix)` call is mounted on the origins of its receiver under its prefix, a
  convention of `CONVENTIONS` (`RequireAuthorization()`, `WithTags(..)`) with none, and the
  application is the root: `WebApplication.Create(..)`, or `Build()` on what
  `WebApplication.CreateBuilder`, `CreateSlimBuilder` or `CreateEmptyBuilder` made, as the
  file's usings (the Web SDK's implicit ones included) spell them. `MapGet`, `MapPost`,
  `MapPut`, `MapPatch`, `MapDelete` and `MapMethods(pattern, methods, handler)` (a list of
  literals or `HttpMethods.X`) register a route; the pattern, the methods and the handler are
  read positionally or by name (`pattern:`, `httpMethods:`, `handler:`, `prefix:`), and `?` of
  `{id?}` is dropped as for controllers. A lambda handler is an inline handler named for the
  first route it serves, a method group (`Handlers.List`) the method it names (one overload),
  anything else the file. A method of the repository hides the framework's `MapGet`,
  `MapGroup` or convention only where it answers the call (`Code::reaches`); `Build()` is the
  framework's when what it is called on comes from its builder. A route above a node whose
  origins climb one call twice (a factory that calls itself to make the group it returns) is
  served nowhere, and so is every route past `MINIMAL_NODES` (4096) nodes in a repository.
  Ceilings: a builder Meada does not read serves nothing (a lambda's parameter, as in
  `UseEndpoints(e => e.MapGet(..))`, a parameter no call reaches, a field, a builder
  reassigned), and so does a group configured through a call outside `CONVENTIONS`; `Map(pattern, handler)`,
  `MapFallback`, `MapHub`, `MapControllers` and `MapRazorPages` are not read, and neither is a
  pattern that does not fold to a literal.
- `http/swift/vapor.rs` (spec #29, #119): Vapor routes, through the same `Mounts` and `prefixes`
  and the origins of a builder as `http/csharp/minimal.rs`. A `grouped(..)` call is mounted on
  the origins of its receiver under its path components (string arguments joined with `/`; one
  that constructs a value is a middleware and adds none, but `String`, `Substring` and
  `PathComponent` construct a path, so those and any other unreadable argument serve nothing),
  and the application is the root: a call that `Vapor.Application` or
  `Vapor.Application.make` names as the file's imports spell it. `get`, `post`, `put`, `patch`,
  `delete` and `on(.METHOD, ..)` register a route whose path components are the string
  arguments (`*` and `**` are one parameter each, `http/swift.rs`), whose handler is the
  trailing closure (an inline handler) or `use:` (a function; `self.f` is `f` of the type
  around the route). Vapor calls `boot(routes:)`, so no call binds its parameter:
  `register(collection: C())` (the type constructed in the argument or in the local it names)
  mounts `Node::Boot`, the `routes` parameter of `C.boot(routes:)`, on the origins of the
  builder it is called on, with no segment; a collection registered on two builders serves each
  route under both. `routes` is followed through a `let` alias and, as any parameter no call
  binds (`Code::origins_unbound`), through the arguments of exact calls. Ceilings as the
  minimal APIs': a builder Meada does not read serves nothing (a parameter no call reaches, a
  `var`, a property), the closure form `group("api") { }` is not read, and a repository past
  `VAPOR_NODES` (4096) nodes serves nothing. Also: a helper `boot` calls
  (`mountReports(routes)`, bare or through `self`) is followed when the call is exact: a member
  `RouteCollection` does not declare is the collection's own (Known outside members), so a
  helper method of the collection is, and the routes it registers serve under the builder it
  was given (a free function helper is exact too, for the same reason);
  a `grouped` whose every argument is a call on a capitalized head with no path producer
  (`String`, `Substring`, `PathComponent`) in its callee (`AuthMiddleware()`,
  `User.authenticator()`) is the middleware overload (`Dialect::middleware`, Vapor only) and
  adds no segment, by shape: a member read argument (`app.sessions.middleware`) leaves the
  group unread, and a static call that returns a path component (`Paths.api()`) is read as
  middleware, so its routes are served under the parent prefix (accepted false positive); an inline handler
  served under two prefixes has one symbol, named by one of them (as in C#); a factory that calls itself serves nothing through
  it, and a builder it reaches by one origin among others serves its routes under the others.
- `http/swift/hummingbird.rs` (spec #29, #120): Hummingbird routes, a `Dialect` that
  `vapor.rs::providers` runs with Vapor's value flow, mounts and handler rules. The root is a
  call that `Hummingbird.Router` names, the child builder is `group(prefix)` (one optional
  string), and `on(path, method:)` takes the path first. Ceiling: there is no
  `RouteCollection`, and a controller's `addRoutes(to:)` is not read, so its routes are not
  served.
- `http/swift/client.rs` (spec #29, #121): a call is a consumer when it is `data(from:)` or
  `data(for:)` on a `URLSession`, `request(_:)` on Alamofire's `AF`, a `Session` or
  `Session.default`, or `execute(_:..)` on an `HTTPClient`. The receiver counts by its spelling
  (`URLSession.shared`, `AF`, `HTTPClient.shared`, as the imports spell them), by a written type
  (`names::typed`) or by a local, property or `let` constant that holds one, followed to
  `CLIENT_DEPTH` (8) names; a repository type of the same name hides the library's. The call is
  read whole by `Expand::Chain` as in `http/kotlin/client.rs`, so a wrapper at any depth (the
  URL, the method or the request a parameter) is a consumer, with cycle detection. The URL is
  `URL(string:)`, `URL(string:relativeTo:)` (RFC 3986 resolution by `http/join.rs`, the resolver
  TypeScript's `new URL(reference, base)` shares (#686): the scheme before any `/?#`,
  network paths, dot segments of every reference, the base's query dropped; a base whose own
  scheme does not lead it is not placed), `appendingPathComponent` (before the
  query), a literal, an interpolation, a `+` or a `let` constant (`static let` too, from other files and
  imported modules); a hole the evaluation cannot fold stays a parameter marker, a reference or a
  base that is a wrapper's parameter is resolved at its call sites, and a URL left unknown is an
  unresolved consumer. UIKit, AppKit, SwiftUI and Cocoa spell Foundation's names (they re-export it). `URLRequest(url:)` takes its method from an `httpMethod`
  assignment, `HTTPClientRequest(url:)` from `method`, Alamofire from `method:`; GET when absent.
  Not read: `dataTask(with:)`, `AF.download` and `AF.upload`, a request mutated through an
  `inout` or an alias, a request with `url` assigned afterwards (unknown URL), a method that is
  not one of the HTTP verbs.
- `http/csharp/clients.rs` (spec #28, #111): an `HttpClient` call is a consumer when its method
  is in `CLIENT_METHODS` (`GetAsync` to `PutAsJsonAsync`) or is `SendAsync`, and its receiver is
  an `HttpClient`: `Code::written` gives the type text written for a field, a parameter or a
  typed local, `names::is_type` confirms it in full or through the usings (an alias counts, a
  class of the repository named `HttpClient` hides the framework's), and a construction in place or a
  local made by `new HttpClient()` counts without a written type. The pack ships syntax only and
  adds no externs: C# types are not in `Facts::externs`, since `Binding::External` would turn
  the calls on them from inferred to unresolved. The call is read whole by `Expand::Chain`, so a
  wrapper (the URL, the method, the `HttpRequestMessage` or the client a parameter) is a consumer
  at each caller with the trunk's depth bound and cycle check; `SendAsync(new
  HttpRequestMessage(HttpMethod.X, url))` takes its method and URL from the request, and
  `new Uri(x)` is `x`. The pack folds string literals, verbatim, raw (one line) and interpolated
  strings (`{{` and `}}` unescaped, a hole with an alignment or format clause is a parameter),
  `+`, and `const` and `static readonly` fields, of other types too (`Facts::consts` and
  `Facts::statics`, as Java's `static final`). The pack binds every local to its initializer
  (`Facts::bound`, an `Unknown` bound for a loop variable, a pattern or a lambda parameter, so
  that a second binding of the name leaves none), and a field or property that holds a
  construction or a call. A binding assigned in its lexical scope (`=`, `+=`, `++`, `out`,
  `ref`, `this.x = v`) loses its bound: it holds no one value (the rule of the minimal API
  reader, spec #28, #110), so a reassigned local, request or client is unknown, and
  `Code::local` follows a local of unknown class when the pack bound it once. A base is the
  `BaseAddress = new Uri(x)` of the client's initializer (the pack gives an initializer as the
  last argument of the creation, a `Value::Object`) or an assignment on the name the call is
  made on (a `Bound` named `<name>.BaseAddress`, in the scope that declares `<name>`, which
  `clients.rs` checks against `Code::written` so that a parameter that hides a field, or a
  nested type's own field, keeps its own); two assignments leave an `Unknown` bound, which is
  an unknown base. The assignment wins over the initializer. A relative URL joins the base as
  RFC 3986 does (`https://c/v1` and `orders` give `/orders`, `https://c/v1/` gives
  `/v1/orders`) and one starting with `/` keeps the origin; a base with no literal scheme is
  cut at the first text after its non-literal parts. `request.Method = HttpMethod.X`, set
  after the construction, is read the same way (a `Bound` named `<name>.Method`). A client with
  no readable base is a path-only contract. Named clients (`CreateClient(name)`) held by a
  local or by a field assigned from it (a `Bound` named `<field>=`, read with `Code::bounds`)
  are not consumers. `a?.M()` is a call of `M` on `a` (the grammar parses `await a?.M()` with
  the `await` around `a`; `condition` reads through it). Ceilings: a field is followed only
  when it holds a construction or a call (an instance `readonly` string is not folded, only
  `const` and `static readonly`); a binding assigned again is unknown in its own scope; sibling methods keep
  independent bindings of the same spelling; `new Uri(base, relative)` with two arguments, a
  `HttpRequestMessage` built with an object initializer and a raw string of several lines are
  not read.
  A member assigned by `??=` or a compound operator, or by an assignment that may not run when
  its definition does (the pack's `Visit::conditional`: inside a branch, a loop, a `catch`,
  `?.` or the right of `&&`, `||`, `??`, reset at each method, constructor and lambda), is an
  `Unknown` bound, and an assignment whose address differs from the initializer's leaves the
  base unknown (the call may come before it).
  Typed clients (spec #28, #113): an instance `readonly` field with no initializer that one
  write of its class sets, a plain `=` in a constructor (`this.f = v`, or `f = v` with no
  parameter or local `f` there; `out` and `ref`, `this.f` too, count as writes), is a constant
  of the class whose value is written in that constructor (`Const::scope`, Java's `final`
  model), so `_http = http` makes `_http` the constructor's parameter (`x ?? throw ..` is `x`).
  The pass groups the writes by class and name once. The pack gives a call's type arguments
  as written (`Call::uses`, one chain `[Step::Argument(tys)]`, the meaning shape facts give
  it), and a target-typed `new(..)` whose declared type it reads (a local, a field or a
  property declaration) is a construction call of that type, so `may_be_called` sees it. The
  kind expands with an injector for `Injected::Param` only: a parameter of a constructor no
  call may reach, declared `HttpClient`, is a client whose base is the one
  `<param>.BaseAddress` the constructor assigns when every registration of its class sets the
  same address or none does, else the one address every framework `AddHttpClient<T>(..)` of
  its class sets (`AddHttpClient<I, T>`: the last type argument; the call must reach no
  function of the repository, `Code::reaches`). A registration's address is the one
  `<param>.BaseAddress` bound of its lambda's parameters (its bindings of no class and no
  value, so a local of the lambda is none), folded and its reads decided where the lambda is
  written; one that only names the client adds none, and a method group, a lambda that passes
  a parameter to a call, or a `ConfigureHttpClient` chained on it (`Call::on`) add an unknown
  one. Two registrations with different addresses, one that does not fold, a constructor some
  call reaches, or a parameter of a method leave the base unknown. The injected value is a
  construction (`new HttpClient { BaseAddress = new Uri(parts) }`), so the base is read as an
  initializer's, and an assignment on the field still wins. Ceilings: a primary constructor's
  parameter (`class T(HttpClient http)`) is a binding of the class, no constructor parameter,
  so it is not injected; a target-typed `new(..)` passed, returned or assigned is no call.
- `http/csharp/refit.rs` (spec #28, #113): a method of an interface (`Facts::interfaces`) with
  Refit's `[Get]`, `[Post]`, `[Put]`, `[Patch]`, `[Delete]` or `[Head]` (through the usings, or
  written as `Refit.Get`, read in the scope around the interface so that a method named like the
  attribute is no type; `Hiding`, shared with the controllers, says a class of the repository
  named `GetAttribute` hides it) and a path from the root is a consumer at its definition
  (`Site::Def`), as Retrofit's are. Its bases are the first arguments of each framework
  `RestService.For<Api>(base)` whose first type argument names the interface or one that
  extends it (#367: the interfaces `Def::bases` reach, up to `BASE_DEPTH` (32) levels, each
  once), read with
  `Expand::Call` (a wrapper's callers pass it) and the reads decided in the file of `For`; one
  consumer per base, and an interface no `For` makes has an unknown base. The route follows the
  base's own path (Refit appends it). `AddRefitClient` and `For` given an `HttpClient` are not
  read. A caller reaches the method through a receiver whose type is declared (a field, a
  parameter, a typed local); `var api = RestService.For<Api>(..)` types nothing, since the
  resolver reads no library's result (the kinds run after it), so its calls stay name matches.
- `http/csharp/reads.rs` and `dotnet/appsettings.rs` (spec #28, #112): environment and
  configuration reads in a consumer's URL. The pack records what looks like a read without
  deciding what it reads: a subscript with one string literal, and a call of
  `GetEnvironmentVariable` (`ENVIRONMENT_READ`, the pack's one library form, on the precedent of
  Java's and Kotlin's `getenv`) with one, is `Value::Str([Part::Read { .. }])` (a
  neutral carrier, since only `Env` and `EnvOr` survive `unbind`; a call of any other member, as
  `MapGroup("/api")`, stays a call). Being a string, a local or a field bound to a read folds
  into the URL like any other. The reads of one node share one `Arc` of their scope. The pack
  folds `??` chains (the first operand a read and no other operand a read, so the reads count
  across the whole chain; past `VALUE_DEPTH` operands it is unknown), `!` around it, and keeps
  the members called on it in `then`, except the trims of `System.String` (`Trim`, `TrimStart`,
  `TrimEnd`: `TRIMS`, the language's own runtime), which leave it the read. `Reads::value` in the kind turns the part into `Env`,
  `EnvOr` or `Hole` with the resolver: a read whose `then` holds a member is a hole, `Environment.GetEnvironmentVariable` is the
  framework's `System.Environment` (a repository type of that name hides it, through
  `names::spellings`), and `x[".."]` is a configuration when `x` is declared `IConfiguration`
  (`Code::written`, confirmed through the usings) or is `builder.Configuration` of a
  `WebApplicationBuilder` (declared, or made by `WebApplication.Create*Builder`). A configuration
  key is the variable `Section__Key`, as the code spells it; the literal that `Settings` finds
  for it is the variable's default (`Part::EnvOr`), so `http::fetches` keeps the twin readings
  Spring's `${X:default}` has: by host when a workspace provider serves the default's host, else
  through the variable and `base_url_env`. `based_on` rebases such a default on a relative URL
  as `Uri` joins. `Settings` reads, from `Indexed::configs` (`**/appsettings.json` beside
  `**/*.csproj`, which the pack's `join` reads), the base file of the project of a source (its
  `Facts::unit`; a file in no project reads the nearest file above it) with serde_json after
  dropping comments, trailing commas and a byte order mark. The parsed document is kept as it is
  and a key `A:B` is looked up by descending its segments without case (cost: segments times
  children), never flattened to full keys; serde_json's recursion limit (`JSON_DEPTH`, 128)
  bounds the nesting, and a file past it is not read. Only strings are values. A value lives
  only during the extraction (`Facts` is not serialized), and a config edit re-extracts the
  consumers through the config digest. Ceilings: a read passed in through another file's
  wrapper call, `using static System.Environment` and a read behind a `??` fallback of a local
  are an unknown part (`ponytail:` marker in `reads.rs`); the variable keeps the key's spelling
  and `base_url_env` compares it as written, though .NET compares keys without case; a JSON key
  that holds a `:` is not matched; `appsettings.<Environment>.json`, `GetValue`, `GetSection`,
  options binding, arrays and user secrets are not read; a chain past `VALUE_DEPTH` is a hole.
