# Capabilities and limits

Meada 1.4 indexes TypeScript, JavaScript, Python, Rust, Go, Java, Kotlin, C# and Swift. It finds calls in code and connects repositories through HTTP, packages and messaging channels. Each link reports its confidence level.

Server mode includes the [console](console.md), [snapshots](snapshots.md), [retention](generations.md) and [managed workspace](workspaces.md) through optional modules. The console has an editor, diff, playground and graph, and a workspace can delegate its entries to [teams](console.md#teams). With `accounts`, the console signs in [local accounts](console.md#local-accounts) with no identity provider: the default account `admin` changes its password at its first sign-in, common passwords are refused, ten consecutive failures lock an account, `meada ui reset-admin` recovers the default account, and the global operator creates accounts, changes their role, suspends, reactivates and resets their password, never leaving the console without an active operator, and gives them groups that `[accounts.groups]` maps to repositories, bounding what each account reads and making team operators of the owners of entries; beside an identity provider, `break_glass = true` keeps only the default account as [break glass](console.md#break-glass). A signed-in account generates [agent tokens](console.md#agent-tokens) for the nodes and, with `proposals`, for the proposal server, valid 1 to 168 hours and shown once, which `meada mcp --listen` and `meada workspaces mcp` with `[accounts]` admit and answers under the account's grant, with no shared bearer. With `[accounts.github]`, a person [verifies their GitHub login](console.md#verified-github-login) once, pasting a personal access token the console uses only to read the login, and their GitHub roles then grant them repositories in the console and on the nodes. The [repository catalog](console.md#repository-catalog) lists, before any manifest exists, only what the person may select. [Compose](https://github.com/ClaudioMascaro/meada/blob/main/deploy/compose/) and [Helm](https://github.com/ClaudioMascaro/meada/blob/main/deploy/helm/meada/) provide deployment examples. The [named workspaces](named-workspaces.md) of `workspaces` can be maintained as versions (save, apply, rollback, rename, delete, delegation) by their operators, through the console and `meada workspaces`, with each change checked against the base it read. Each workspace may declare its own peers, environment-variable references, resources and serve hosts for a shared repository, and the console shows the [effective configuration](named-workspaces.md#effective-configuration) (what a repository's own `meada.toml` overrides, what is not yet in effect) before the text is saved. Operators assemble that manifest with [guided controls](named-workspaces.md#guided-authoring) (repositories from the catalog, peers, serve settings, resources, environment-variable names) that stay one document with the text, showing the diff and freezing the reviewed payload until they confirm. With `proposals`, a [proposal server](named-workspaces.md#mcp-assisted-authoring) (`meada workspaces mcp`) lets a CLI agent, with its own GitHub read MCP, list what the operator may select and validate a manifest as the console would save it, writing nothing, for a person to review and apply in the console. Named workspaces and the proposal server also run with local accounts, with `[accounts.github]` in place of `[oidc.github]`.

The [console graph](console.md#infrastructure-in-the-graph) shows provisioners and their roots,
provisioning links, image builder → runner and alert emitter → matcher links. Its legend explains
the roles; accessible named tables support keyboard navigation, retain link levels and reasons,
and show sides without a counterpart. The playground offers `value`, `iac`, `image` and `alert`
searches.

## Languages and resolution

| Language | Files read |
| --- | --- |
| TypeScript and JavaScript | `.ts`, `.tsx`, `.js`, `.jsx`, `.mjs`, `.cjs` |
| Python | `.py`; each `pyproject.toml` gives source roots (its directory, its declared roots or `src`) to the files under it |
| Rust | `.rs` reachable through each crate's module tree |
| Go | `.go` included by the package's build rules |
| Java | `.java`, resolved by their `package` declarations |
| Kotlin | `.kt` and `.kts` (but `build.gradle.kts` and `settings.gradle.kts`), resolved by their `package` declarations |
| C# | `.cs`, grouped into projects by SDK-style `.csproj` files and resolved by their `namespace` declarations |
| Swift | `.swift` (except `Package.swift` and `Package@swift-<version>.swift`), grouped into modules by the targets of `Package.swift` |

An annotation (Java, Kotlin), an attribute (C#, or a Rust route attribute) or a decorator (Python) written on a definition is a call of the enclosing scope, at the level it resolves to. The export's call site names the definition it decorates (`decorates`), and `context` lists it apart, under `annotations:`, not among the callees.

### TypeScript and JavaScript

Meada reads functions, classes and methods, `const`, `let` and `var` bindings of a function or class, and `export default`. It also reads functions defined by assignment at the top level of a file: `obj.y = function () {}` defines `obj.y`, `X.prototype.y = function () {}` defines the method `X.y`, `exports.y = ...` and `module.exports.y = ...` define `exports.y`, exported as `y`, and `module.exports = function () {}` defines the default export. A `let` or `var` is a definition only if the file never assigns that name again. Functions and methods in the object literal of a top-level `const` define `o.m`; options objects written as an argument keep their inline definitions. A call `obj.y()` or `y()` in the same file reaches these definitions, apart from `exports.y`, which a bare `y()` does not reach (Node binds no local `y`). `require` and `module.exports` are not imports between files, and `new F()` of a constructor function does not reach `F.prototype` methods. An import uses the `paths` and `baseUrl` of the nearest `tsconfig.json` (or `jsconfig.json`), in the file's directory or above; both are relative to that config's directory, and `node_modules` is ignored. When that config has an `extends` to another `tsconfig.json` or `jsconfig.json` of the repository (a relative path), `paths` and `baseUrl` come from the chain, the nearest config that declares each option winning; an `extends` to a package or outside the repository is not read, and a chain that is too long or circular counts as the nearest config alone.

### Rust

Each `Cargo.toml` with `[package]` defines a crate, in any directory. Meada follows its `mod` declarations without running the Rust toolchain. It reads functions, types, traits, modules and `impl` methods. A file no `mod` reaches stays out.

Calls such as `crate::a::f()`, `Type::new()` and `Self::f()` follow their written paths. Resolution also follows `use`, groups, renames, globs, `pub use` and other workspace crates by name.

For `x.m()`, Meada uses the receiver type written in a `let`, parameter, field, `self` or function return. A call on another call's result (`Router::new().route(..)`, also through `.await`, `?` or parentheses) resolves through the inner call's written return type: through `?`, the type a `Result<T>` or `Option<T>` wraps, and through `.await` on a function that returns `impl Future<Output = T>`, `T`. A `dyn Trait` or trait-bounded generic reaches the trait's method. Without a written type, resolution uses the name. Methods absent from the type's `impl` blocks, including those generated by `derive` or supplied by external traits, produce no link. Route attributes do not link to functions either.

Analysis uses each crate's default configuration for x86_64 Linux. Items with a false `#[cfg(..)]` and features outside `default` stay out; modules compiled in a test build are included, not only `#[cfg(test)]` ones. Calls inside macros are not indexed, with one exception: the braced body of an item macro invoked in a module, or among the items of an `impl` or a trait, is read as items of that module or associated items of that block (functions, types, `impl` blocks, `use`, `mod` and the calls inside them); the invocation is written `m!` or `crate::m!`, and any other path is skipped. An item macro is a `macro_rules!` of the repository itself whose first arm takes `$($x:item)*` and writes each item back with only outer attributes before it, as tokio's `cfg_rt!` does; the `#[cfg(..)]` attributes the arm writes apply to every item. When the name has several definitions in the repository, all of them must agree on the attributes. Other macros, those of other crates included, stay unread. Rust contracts cover packages, HTTP and channels.

### Go

AWS SDK for Go v2 clients count when made by `New` or `NewFromConfig`, including aliases and factories followed through exact calls. SQS, SNS, EventBridge, Kinesis, S3, DynamoDB, SSM and Secrets Manager use the AWS SDK forms below. A declared type alone supplies no client origin, except an SQS client declared `*sqs.Client`, which keeps the SQS forms Meada read before.

The `module` path in each `go.mod` resolves imports to package directories, including `replace` to a repository directory. A nested module owns its subtree. Files in one directory with the same `package` share a scope, including `_test.go`. Meada does not run the Go toolchain.

Analysis targets linux/amd64. Files named `_windows.go`, with a false `//go:build`, or using cgo (`import "C"`) stay out. Meada reads functions, types, interfaces and methods (`Type.Method`). It resolves calls within the package and through imports (`pkg.F()`, alias or dot); standard-library and external modules remain external.

A `recv.M()` call is `exact` when the code supplies its type in a parameter, field, `var`, composite literal or function result. Otherwise, name resolution is `inferred` or `ambiguous`. A call through an interface stops at its interface method.

### Java

AWS SDK for Java 2.x recognizes synchronous and asynchronous clients made by `create()` or `builder().build()`, with builder requests. SQS, SNS, EventBridge, Kinesis, S3, DynamoDB, SSM and Secrets Manager use the AWS SDK forms below. An injected type alone supplies no client origin; factories must resolve through exact calls.

Meada reads classes, interfaces, enums, records, methods and constructors, including nested types. Names include the package and, for methods, the written parameter types: `example.Orders.place(String,int)`. Calls in lambdas and anonymous classes belong to the enclosing symbol.

Single-type, package and static imports, including `*`, resolve through package declarations. Bare, `this` and static calls are `exact` when argument count leaves one method, counting varargs; overloads with the same argument count stay `ambiguous`. External types stay external.

A call on a field, parameter or local variable whose type is written resolves within that type and the supertypes written in `extends` and `implements`, and so do `var x = new T(...)`, `var x = f()` and `f().g()` through the declared return type of `f`, `super.f()`, and bare and `this` calls. Each signature comes from the first class that declares it, so a class's `f(int)` overrides its base's and an inherited `f()` stays visible. A call through an interface-typed receiver stops at the interface method. A supertype from outside the repository has unknown members: with one anywhere in the hierarchy, none of its methods is `exact`, and the call resolves by name within the hierarchy. The exception is the platform types whose members are fixed and known (`Serializable`, `Cloneable`, `Runnable`, `Comparable`, `AutoCloseable`, `Closeable`; in Kotlin also `kotlin.Comparable`): they hide only the names they declare, so a class that `implements Serializable, Comparable<T>` gets `exact` calls to its own overloads, and only `compareTo` resolves by name. With an unknown receiver, the call resolves by name: `inferred` when one method has that name in the repository or, other than the calling method itself, in the calling class's hierarchy, `ambiguous` with every candidate when several do.

Public types and their public or protected members form the exported surface.

A file is skipped as too broad when the enclosing names it repeats (charged at every node under them) pass 4 MiB in all: it has no symbols or calls, other files index, and `meada status` names it. Indexing needs no JDK, Maven or Gradle and executes no repository code.

### Kotlin

Meada reads classes (including `data`, `sealed`, `enum` and `annotation` classes), interfaces, objects, companions, top-level functions, extension functions, properties with getters and methods, including nested ones. Names include the package and, for functions, the written parameter types: `example.helper(Int)`. An unnamed companion is `Class.Companion`, and an extension function carries its receiver type: `example.Point.norm()`. Calls in a lambda belong to the enclosing symbol. In a method, an extension function or a scope block, a bare call takes an extension of the implicit receiver that is in scope before a top-level function of the same name, as Kotlin resolves it (`exact`, or `ambiguous` with the function when the extension may apply to any receiver); an `object`'s name is its instance, so `Dispatchers.setMain(..)` reaches an imported extension of `Dispatchers`, while a class's name keeps companion and statics.

Single, aliased (`as`) and `*` imports resolve through package declarations, and the file's own package is visible without an import; a name two `*` imports both give is `ambiguous`. Bare, `this`, object (`Registry.find()`) and companion (`Point.origin()`) calls are `exact` when argument count leaves one function, counting parameters with default values and `vararg`; overloads with the same argument count stay `ambiguous`. Calling a class's name constructs it: the call reaches the primary or a secondary constructor, chosen by argument count. A call on another call's result (`f().g()`, `Box().g()`) resolves through the written return type of `f` (`Box?` and `Box<T>` name `Box`), or the class a constructor builds; with no written type (`fun f() = Box()`) or a type from outside the repository it is not `exact`. An `expect` class and the `actual` classes of its name are one class, the `expect` one: the members it declares resolve to it, and a member only an `actual` declares resolves by name only. Inside a lambda, a bare or `this` call may belong to an implicit receiver, and resolves by name only. Kotlin and Java in one repository call each other by fully qualified name. Names outside the repository stay external.

Declarations that are neither `private` nor `internal` form the exported surface; an `internal` one is visible across the repository, and a top-level `private` one in its own file only. A file is skipped as too broad when the enclosing names it repeats (charged at every node that stores one) pass 4 MiB in all: it has no symbols or calls, other files index, and `meada status` names it. Indexing needs no Kotlin compiler, JDK or Gradle.

### C#

Concrete AWS SDK for .NET clients count through `new`, including aliases and exact factories. Request initializers, named arguments, lists and dictionaries name the sides of SQS, SNS, EventBridge, Kinesis, S3, DynamoDB, SSM and Secrets Manager. An injected interface type alone supplies no client origin.

Each SDK-style `.csproj` (one with an `Sdk`) is a project and holds the `.cs` files of its directory, inside a `.sln` or not; `.sln` files change nothing, and an old-style `.csproj`, with no `Sdk`, is no project. Meada reads classes, records, structs and interfaces, nested ones (`Outer.Inner`) and `partial` ones included (each part in its own file, with every part's members), methods and constructors with their written parameter types (`Acme.Orders.OrderService.Place(string,int)`) and local functions (`Method.Local`). Block and file-scoped namespaces give the full names. A type two projects of the repository declare under one full name is, in a file of one of them, that project's own, as the compiler picks it; in a file of another project, it is the one of the project its own references (`ProjectReference`, directly or not), and the name is `ambiguous` when both or neither are referenced. An `internal` type (one with no access modifier too) exists for the files of its own project alone: in a file of another project the name binds to the type that file sees, as if the `internal` one did not exist, and a call to it gets no link; so does an `internal`, `private` or `private protected` constructor or method of a public type (`InternalsVisibleTo` is not read). The `obj/` and `bin/` directories beside a `.csproj` are build output and stay out of the index, even with no `.gitignore`. A lambda's calls belong to the symbol around it, and an attribute is a call on what it decorates.

`using`, `using static`, aliases (`using X = A.B`) and `global using` resolve through the namespaces of every project; a `global using`, the `.csproj`'s `<Using>` items and enabled implicit usings (`ImplicitUsings`) apply to every file of the project, and to it alone. The types of every namespace around the file's are visible without a `using`, the innermost first (in `Acme.Query.Internal`, those of `Acme.Query.Internal`, `Acme.Query` and `Acme`) and before a `using`'s, in file-scoped and nested block namespaces alike. Bare, `this`, `base`, static (`Type.Method()`) and `using static` calls (which reach only the members and nested types the named type declares, not its bases') are `exact` when argument count leaves one method, counting `params` and parameters with default values; overloads with the same argument count stay `ambiguous`. Types outside the repository stay external. Calls on a receiver of a written type do not resolve through that type yet. A base from outside the repository keeps calls to the class's members from `exact`, except `IDisposable`, `IAsyncDisposable`, `IEquatable` and `IComparable`, whose members are known: they hide only the names they declare. The items (`<Using>`, `PackageReference`, `ProjectReference`) and properties of a `Directory.Build.props` apply to the projects below it as if written in the `.csproj`, evaluated before the project's own (the nearest `props` hides those above it unless it imports its parent with `GetPathOfFileAbove`); a `ProjectReference` there is relative to each project, and one built on `$(MSBuildThisFileDirectory)` counts as computed. A `props` file with more than 256 `<Using>` items and references is not read. `nameof(x)` is the text of the last identifier of `x` (`nameof(Orders.Topic)` is `"Topic"`), so a channel name or a URL part written with it is known.

Public types and their public or protected members form the exported surface. A file is skipped as too broad when the enclosing names it repeats (namespace, types and method, the part of the whole enclosing name over 256 bytes, charged at every node under it) pass 4 MiB in all: it has no symbols or calls, other files index, and `meada status` names it. A project whose files get more than 256 distinct usings (`global using`, `<Using>` items and implicit usings together) has every one of its files skipped the same way, and `meada status` names each with the limit. Generated files with ordinary names, such as Entity Framework model snapshots, read whole at any size. Indexing needs no .NET SDK or MSBuild and runs no repository code.

### Swift

Each target of `Package.swift` (`.target`, `.executableTarget`, `.testTarget`, `.macro`, `.plugin`) is a module of its package (two packages of the repository that name a target alike give two modules) holding the `.swift` files under its literal `path:`, or under `Sources/<name>` by default; the files outside every target, all of them in a repository without `Package.swift`, form one module. Meada reads classes, structs, enums, actors and protocols, nested ones included (`Outer.Inner`), and functions, methods and initializers named with their argument labels (`OrderService.place(order:quantity:)`, `init(baseURL:)`); declarations of one scope in a file (a type's body and its extensions) with the same labels and other types carry their types too (`audit(_:Order)`). The members of an extension belong to the extended type, in any file of the module, and those of a protocol extension are the default implementations of its requirements. The calls inside a closure belong to the symbol around it.

Names resolve within the file's module first, then within the modules it imports (`import Store`), through their `public` and `open` declarations, and through the `internal` ones too when the import is `@testable`; `import Store` names the target of the importer's own package, else of the nearest package in the directory tree (every one when several are as near, so a call through it is `ambiguous`); a module that is no target of the repository is external, and an `internal` declaration exists only in its module and in the files that import it `@testable` (`fileprivate` and `private` ones, in none other). Unqualified, `self`, static (`Type.method()`) and initializer calls (`Type(...)`, or the type itself when it declares no initializer) are `exact` when one declaration fits the labels, counting parameters with default values, variadic ones and trailing closures; a trailing closure fills only a parameter of a function type (also optional, or with attributes such as `@escaping`), of a generic parameter or of a repository `typealias` of a function type, never one of a type the repository declares or of another written type (`Wheel?`, `[Int]`); a parameter written as a name from outside the repository (`animation: Animation?`, `Ext.Handler`) may be an alias of a function type, so a call that fits an overload only through its trailing closure is `inferred`, or `ambiguous` among several, never `exact` (a parameter before the one the closure fills anyway is no matter); overloads with the same labels stay `ambiguous`. A function with a capitalized name is called as any function, and a call not written under `await` does not reach a function declared `async`. A call on a property, a parameter or a local whose type is written (`let x: T`, `x: T?`, `any P`, or a `typealias` of it, such as `StoreOf<X>` for `Store`, followed from alias to alias up to a limit) or that is initialized with `Type(...)` resolves within that type, its extensions and the protocols it conforms to, `exact` when one declaration fits the labels; through a protocol-typed receiver it stops at the requirement, and conforming types are not searched. A name bound twice in a function, or by a closure parameter or a pattern, or named like a type parameter, has no known type there. A call on a receiver whose type is not read resolves by name and labels (`inferred` or `ambiguous`). A call on an implicit member (`.name(..)`, as in `wait(.seconds(1))`) resolves the same way, among members and never to a function or a type of that name; `.init(..)` stays unresolved. A receiver of a type from outside the repository (`session: URLSession`, `String`, or a capitalized name nothing in the repository declares, such as `AF`) reaches only the members of that type's extensions in the repository (`extension URLSession { }`), and a call past a member of such a name (`URLSession.shared.data(..)`), or on a value bound to that member (`let session = URLSession.shared`), does not resolve. A receiver or a path head written as a `typealias` of such a type (`typealias Anim = Animation`, also through aliases of aliases) reaches the same extensions. As in Java, a type with a base from outside the repository (`NSObject`, `View`) gets no `exact` calls to its own overloads, unless the base is a type with known members (`Sendable`, `Error`, `Equatable`, `Comparable`, `Hashable`, `Encodable`, `Decodable`, `Codable`, `Identifiable`, `CustomStringConvertible`, `CaseIterable`, an enum's raw value type or Vapor's `RouteCollection`), which hides only the names it declares. A file is skipped as too broad when the enclosing names it repeats (charged at every node that stores one) pass 4 MiB in all: it has no symbols or calls, other files index, and `meada status` names it. Indexing needs no Swift toolchain or Xcode.

## HTTP and contract documents

IaC declarations also provide HTTP routes: API Gateway HTTP and REST in Terraform/OpenTofu (HCL and JSON), SAM `Api`/`HttpApi` events, and Serverless `http`/`httpApi` events. Literal methods and paths use the same parameter normalization as code routes. The provider points to the local handler symbol when it resolves with certainty, or to the whole repository. Consumers in other repositories use ordinary host and peer rules. Unknown references, methods or paths never invent routes; non-Lambda integrations and catch-all routes without a finite path stay out of the map (#666).

Base URLs from configuration or a workload environment can name a Kubernetes Service
(`orders`, `orders.shop`, `orders.shop.svc` or `orders.shop.svc.cluster.local`, with or
without a port) or a Compose service. Meada follows the Service selector and the workload's
image or local code path to the repository serving the route. These links are `inferred`,
with the Service in the reason. Short names use the known workload's namespace and evaluation
root; distinct candidates remain `ambiguous`. Environment values come only from indexed
entries of the workload running that code.

An Ingress joins its host and literal prefix to the Service's route, like a gateway prefix.
The most specific prefix wins. Rules with a rewrite annotation, an unknown backend or a
`pathType` other than `Prefix` remain unlinked (older forms without `pathType` are accepted).
Controller-specific paths, regular expressions and wildcard hosts are not interpreted.
No cluster is queried. When neither the Service nor the Ingress reaches code (a Service that
selects no indexed workload, an unlinked rule), the host is left to `[serve] hosts`: a host
declared there still links `exact`.

Meada connects consumers and providers across languages, including behind gateways. A route whose handler is in a test file (`*_test.go`, `*.test.*`, `*.spec.*`, `test_*.py`, a `tests` directory or `src/test`) is no provider unless `meada.toml` sets `test_routes = true` (see [Configuration](configuration.md)):

| Language | HTTP consumers | HTTP providers |
| --- | --- | --- |
| TypeScript / JavaScript | `fetch`, Backstage `discovery.getBaseUrl` and `proxy.endpoints` (also the legacy shape, the keys of `proxy` that start with `/` when no file has `endpoints` nor an `$include` there, at `proxy` or at the root that Meada does not follow, and an endpoint written as a string, `'/ep': https://...`) | Next.js app router, Express, Backstage backend plugins |
| Python | `requests`, `httpx`, `aiohttp` | FastAPI, Flask, Django |
| Rust | `reqwest`, `ureq` | axum, actix-web, Rocket |
| Go | `net/http`, resty | `net/http`, gin, echo, chi, gorilla/mux |
| Java | RestTemplate, WebClient, `java.net.http`, OkHttp, OpenFeign, Retrofit | Spring MVC, JAX-RS |
| Kotlin | RestTemplate, WebClient, OkHttp, Retrofit, Ktor client | Spring MVC, Ktor |
| C# | `HttpClient`, typed clients, Refit | ASP.NET Core (controllers, minimal APIs) |
| Swift | `URLSession`, Alamofire, AsyncHTTPClient | Vapor, Hummingbird |

A URL whose base Meada could not read, with a path appended to it, is an unresolved consumer: an open item that names the limit (the return value of a call, with its callee; a parameter no call binds, with its name; the base of a client Meada did not read, such as the unknown bases of this section; or another part it did not read). That base may hold a path of its own, so the candidates are the routes of the call's method whose path ends with the known part at a `/`. The path stays certain, and the link `inferred` with the reason `host unresolved`, when the URL has no base or when the library's join replaces the base's path with a path from the root: `new URL('/x', base)` in TypeScript, a request URI from the root on a C# `HttpClient`, a Retrofit route from the root, a request from the root on a Ktor client with `defaultRequest` or on an aiohttp session, and `URL(string:relativeTo:)` in Swift. An environment read written in place still links by its variable.

In TypeScript and JavaScript, a `fetch` whose URL is built by a function of the repository is a consumer when the function only returns a string: an arrow function with an expression body, or a body of one lone `return`. The call (`fetch(url("api/vets"))`), or a constant holding it (`const u = url("api/owners"); fetch(u)`), folds to the string with the function's parameters replaced by the call's arguments, also when the function comes from another module. Only a call that resolves exactly folds. A function of more than one statement, an `async` one, one that returns anything but a string, and a string with a part that does not fold (a base read from `process.env`, a conditional over a build-time global) leave the consumer unresolved.

In TypeScript and JavaScript, `new URL(x)` with one argument reads as `x`, as the WHATWG URL Standard has it, only for the unassigned global `URL` or the named `URL` export of `url` or `node:url` through an import or unshadowed `require`. A constructor from an unknown package, repository alias or another export stays opaque. `u.toString()`, `u.href` and `String(u)` pass the URL through, and so does `${u}` in a template string, whether the URL is in a constant or built in place (`fetch(new URL(BASE + "/api/vets").toString())`), and a client wrapper called with one folds at its call sites. A base read from the environment shows its `via`. Changes through `u.searchParams`, `u.search` and `u.hash` keep the path; a write to any other member of a constant built by `new URL` (`u.pathname = ..`, `u.host += ..`), anywhere in the file, leaves the consumer unresolved, and so does a mutation inside a function the URL is passed to, which Meada does not see. `new URL(ref, base)` with two arguments is the join of both, as the WHATWG URL Standard has it (RFC 3986 section 5.2 for these references): an absolute `ref` is itself, `//host/x` takes its host from `ref`, `/orders` replaces the base's path, and `orders` joins the base's directory (`https://h/api/` gives `/api/orders`, `https://h/api` gives `/orders`). Over a base read from the environment or a hole, `/orders` keeps the base as its lead (`http::GET::/orders`, with `via env:API_URL` for `process.env.API_URL`); a relative `ref`, one that starts with a read, and a query or a fragment alone leave the consumer unresolved, and a parameter on either side is resolved at the call sites, as in a client wrapper. `.href`, `toString()` and `String(u)` on a two-argument `new URL` are not read.

`String(u)` respects local bindings of `String` and passes the URL through only when the call uses the global conversion. Assignments and increments that can replace that conversion leave the URL unresolved. A local binding preserves the global conversion outside its scope. Block functions in non-strict JavaScript also hide the conversion in the enclosing function; modules, classes and `use strict` preserve block scope.

In Java, a method of a `@RestController` or `@Controller` class with `@GetMapping`, `@PostMapping`, `@PutMapping`, `@PatchMapping`, `@DeleteMapping` or `@RequestMapping(method = RequestMethod.X)` becomes a provider under the class's `@RequestMapping` prefix. Paths fold `static final` constants (also from other classes) and `+`; each path of an array is one provider. A `@RequestMapping` without `method` serves every method (`http::ANY::<path>`) and links to consumers of any method, with "any method" in the reason. `server.servlet.context-path` is not applied. Annotations and `RequestMethod` count through a Spring import, single-type, static or `import ...bind.annotation.*`; a repository class with the same name, or an import that could also bring the name, hides them. Annotations may also be written in full without an import (`@org.springframework.web.bind.annotation.GetMapping`), unless the repository declares a class by that name. In `method` or a path array, an unreadable element is ignored and the others count.

In Java, a class with `@Path` (`javax.ws.rs` or `jakarta.ws.rs`) is a JAX-RS resource: a method with `@GET`, `@POST`, `@PUT`, `@PATCH`, `@DELETE`, `@HEAD` or `@OPTIONS` becomes a provider at the class path joined to the method `@Path`, with constants folded and `{id}` or `{id:regex}` as the parameter. A method with only `@Path` (a sub-resource locator) is no route, and `@ApplicationPath` is not applied. Names count through an import, as in Spring. A controller or resource may extend a class from outside the repository (`extends com.corp.Base`): its method annotations are read through the imports, and only a nested type of that base named like the annotation would be read wrong. A `@Path` interface is a client, not a server. Annotations written in full without an import are read; annotations inherited from an interface are not.

In Kotlin, Spring controllers follow the Java rules, and a controller may extend a class from outside the repository. A Ktor route inside `routing { }` (`get`, `post`, `put`, `patch`, `delete`, `head`, `options`, with or without a path, and `route(path, HttpMethod.X)`) becomes a provider under the prefix of every `route(prefix) { }` block around it, also when it sits in an extension function on `Route` or `Routing` called inside the block, in another file: called under two prefixes, it serves under both. The route's lambda is its handler. Ktor names count only with Ktor's import; type-safe routing (resource classes) is not read.

In Swift, a Vapor route (`get`, `post`, `put`, `patch`, `delete` and `on(.METHOD, path)`) called on the application (`Application` or `Application.make`) or on a `grouped(prefix)` of it is a provider under the prefix of every `grouped` around it, also when the route builder goes through variables, function arguments and return values. A `RouteCollection` registered with `register(collection:)` serves the routes of its `boot(routes:)` under the builder it is registered on, and under each one when registered on two. The trailing closure is the handler, and `use:` takes a function (`self.f` is `f` of the type around it). A `grouped(..)` whose every argument is a call on a capitalized head that produces no path (`AuthMiddleware()`, `User.authenticator()`) is the middleware overload and adds no path; a member read (`app.sessions.middleware`) leaves the group unread, and a static call that returns a path component (`grouped(Paths.api())`) is read as middleware, so its routes are served under the parent prefix (an accepted false positive), and a path Meada does not read makes no route. The names hold only with the `Vapor` import.

In Python, `base_url_env` also links the base URL of `os.environ`, `os.getenv` and of a field of a `BaseSettings` subclass (pydantic-settings) in the same file: the variable is `<env_prefix><FIELD>` in upper case (the literal `env_prefix` of `model_config = SettingsConfigDict(...)` or `class Config`), or the field's `alias` / `validation_alias`, unprefixed. Meada never reads a variable's value.

In TypeScript, `base_url_env` links the base URL of `process.env.X`, `process.env["X"]` and `import.meta.env.X`, also through `||` and `??` fallbacks and string methods such as `trim` or `replace`, and that of Backstage's `config.getString('a.b')` or `getOptionalString('a.b')` whose key holds exactly `${VAR}` in an `app-config*.yaml` of the repository (`app-config.yaml` first, the first file holding the key winning), also when the value comes from a partial file through `$include` (with or without a key path after `#`, from the including file's directory, a partial including another) or is a `$env: VAR`. An `$include` with an absolute path, one that leaves the repository, through a symbolic link, with `${`, of an extension other than `.json`, `.yaml` and `.yml`, to a file or key path that does not exist, with another key beside it, in a cycle, past 8 includes deep, 64 includes or 1 MiB grafted from one `app-config` is not followed: the keys under it stay unread and `meada status` names it with its reason. `$file` is not read. An `app-config` that is one `$include` is the file it names, and one of more than one YAML document is not read, as Backstage refuses it. The read can sit in a constant, in the URL itself, or in a `readonly`, `private` or `#x` class field written once, by its initializer or by one plain `this.field = value` in the constructor body: `this.field` in a method is that read. A public mutable field, one written twice (a conditional write counts as two) or outside the constructor, a parameter property, a `this.field = parameter` and a field of a JavaScript file stay without a value. Meada never reads a variable's value.

The `context` of a function or method shows the level, reason and provider of inferred HTTP call links when the base URL's environment variable selects a peer through `base_url_env`, such as `base URL from API_URL` and the selected peer. This includes a base read through a field or a Backstage partial; editing only the partial updates the reason and peer on the next query. `trace` shows the same reason.

In Swift, a Hummingbird route (`get`, `post`, `put`, `patch`, `delete` and `on(path, method: .get)`) called on a `Router()` or on a `group(prefix)` of it is a provider under the prefix of every `group` around it, at any depth, also when the router goes through function arguments and return values. The route closure is the handler, and `use:` takes a function. The names hold only with the `Hummingbird` import; a controller's `addRoutes` is not read.

In C#, a minimal API is a provider: `MapGet`, `MapPost`, `MapPut`, `MapPatch`, `MapDelete` and `MapMethods(pattern, methods, handler)` on the application (`WebApplication.Create`, or the `Build()` of `WebApplication.CreateBuilder`) or on a `MapGroup(prefix)` group, with groups nested at any depth. The prefix follows the group through locals, method arguments, return values and extension methods called on the group (`group.MapOrders()`, with the static class's namespace imported), so an extension method called on two groups serves under both prefixes. `RequireAuthorization()`, `WithTags(..)` and the other common conventions keep the group. A lambda handler is an inline handler named for its route; a method group (`Handlers.List`) is the method it names. A group Meada does not read (a lambda's parameter, as in `UseEndpoints(e => e.MapGet(..))`, a field, a reassigned local) gives no route, not even at the root.

In C#, a method of a controller class (with `[ApiController]`, or deriving from `ControllerBase` or `Controller`) carrying `[HttpGet]`, `[HttpPost]`, `[HttpPut]`, `[HttpPatch]`, `[HttpDelete]`, `[HttpHead]` or `[HttpOptions]` becomes a provider, under the class `[Route]`. `[controller]` becomes the class name without its `Controller` suffix, `[action]` the method name, and `{id}`, `{id:int}` and `{id?}` become a parameter. A template starting with `/` or `~/` replaces the class prefix. A method-level `[Route]` without a method attribute serves every method (`http::ANY::<path>`) and links to consumers of any method, with "any method" in the reason. Attributes and base classes count through `using Microsoft.AspNetCore.Mvc` or their full name (`HttpGet` and `HttpGetAttribute` are one name); a class of the repository with the same name hides them. A template must be a string literal, and a literal bracket is written `[[` or `]]`; any other `[token]` gives no route. `[action]` loses its `Async` suffix (as ASP.NET Core does by default) or is the literal `[ActionName("x")]`. With no class `[Route]` and no method template the route is conventional (`MapControllerRoute`), which is not read; `[Route("")]` is the root. Only public methods without `[NonAction]` count. A `[Route]` that is not MVC's leaves the class or method without a route. A controller deriving from another class of the repository, a class that is the base of another one, and a class declared in parts (`partial`) across files give no route.

In C#, a call of `GetAsync`, `PostAsync`, `PutAsync`, `PatchAsync`, `DeleteAsync`, `GetStringAsync`, `GetStreamAsync`, `GetByteArrayAsync`, `GetFromJsonAsync`, `PostAsJsonAsync`, `PutAsJsonAsync` or `SendAsync(new HttpRequestMessage(HttpMethod.X, url))` on an `HttpClient` is a consumer. The receiver counts only when it is an `HttpClient` by its declared type (a field, parameter or typed local) or construction, resolved through the usings, never by the name alone. The URL folds literals, interpolation (`$"{base}/users/{id}"`), verbatim and raw strings, `+`, and `const` and `static readonly` fields, also from other classes; a part that cannot be evaluated is a parameter. A local bound once (`var url = $"..."`) is followed; a binding assigned again holds no one value in its own scope; the same spelling in another method remains independent. `BaseAddress = new Uri(base)` on the client, in its initializer or by assignment (once), is the host of its relative URLs, joined as `Uri` does (`https://c/v1` and `orders` give `/orders`), and a method that takes the path (or the method, or the client) is read at each of its callers. A URL that cannot be evaluated is an unresolved consumer. Clients from `IHttpClientFactory.CreateClient(name)` are not read.

In C#, a typed client registered with `AddHttpClient<T>(c => c.BaseAddress = new Uri(base))` (also `AddHttpClient<I, T>`) gives that base to the `HttpClient` the container passes to `T`'s constructor and to every call on it inside `T`, also through a `readonly` field the constructor assigns. The base folds like a URL and may come from an environment or configuration read; a `BaseAddress` the constructor sets on its parameter wins. Two registrations with different bases, a base that does not fold, a registration Meada cannot read (a method group, a lambda that passes the client on, a chained `ConfigureHttpClient`), a `BaseAddress` the constructor sets conditionally or that differs from the registration's, or a constructor the code calls itself (`new T(..)`, a typed `T t = new(..)`) leave the base unknown. A primary constructor's parameter (`class T(HttpClient http)`) is not injected. Each method of a Refit interface with `[Get("/x")]`, `[Post]`, `[Put]`, `[Patch]`, `[Delete]` or `[Head]` (through `using Refit`) is a consumer, with the base of each `RestService.For<T>(url)` whose `T` is the interface or an interface that extends it, the path appended to the base's own path; its callers reach it through a receiver whose type is declared (a field, a parameter, a typed local), so `trace` goes from the caller through the interface method to the provider; `var api = RestService.For<T>(..)` does not type `api`. An interface no `RestService.For` makes, such as one registered with `AddRefitClient`, has an unknown base.

In Swift, a `data(from:)` or `data(for:)` call on a `URLSession`, `AF.request(url, method:)` (or on a `Session`) and `execute` of an `HTTPClientRequest(url:)` with `method` are consumers; the method is GET when not written, and an assigned `httpMethod` of a `URLRequest` becomes the method. The URL folds `URL(string:)`, `URL(string:relativeTo:)`, `appendingPathComponent`, literals, interpolation, `+` and `let` constants (`static let` too, from other files and modules). A function that passes the URL, the method or the request on as a parameter is a wrapper, at any depth; an unreadable URL is an unresolved consumer. `dataTask(with:)` is not read.

In Kotlin, a Ktor client call (`get`, `post`, `put`, `patch`, `delete`, `head`, `options` and `request(url) { method = HttpMethod.X }`, `GET` when no method is set) on an `HttpClient` is a consumer. A `defaultRequest { url(base) }` in the client's configuration is the base of its relative URLs. The URL folds literals, templates (`"$base/users/${id}"`), `+`, `const val` and `val` properties initialized with a foldable expression, also from other files; a part that does not fold becomes a parameter. A function that wraps the client becomes a consumer at each call, with its arguments substituted, through any number of levels; a call whose URL cannot be evaluated is an unresolved consumer, and `url { path(..) }` is not read. The method the `request` block assigns (`method = m` or `this.method = m`) comes from the wrapper's argument at each call; an assignment that may not run (an `if`, a lambda inside the block), a block that is no lambda written in the call or a method that is no `HttpMethod` member leaves the method unknown, never `GET`. The client may be imported by `*`, written by its type, or held by an untyped local `val` (`val c = HttpClient(CIO)`, `HttpClient(CIO).use { it.get(..) }`); a `var` is not read, and a local named like a client of its file hides it, with its own base. Names count only with Ktor's import and a receiver that is an `HttpClient`.

In Kotlin, `RestTemplate`, `WebClient`, OkHttp and Retrofit follow the Java rules, written the Kotlin way: `RestTemplate()` and `Request.Builder()` without `new`, named arguments, `Api::class.java` as the Retrofit service, and a Retrofit interface of annotated `fun`s (the annotations may come from `retrofit2.http.*`). The client may be held by a property, a constructor property or a local `val` (`val x = RestTemplate()`, `val web = WebClient.create(..)`), and its base is the one of the call that made it. The extension `retrofit.create<Api>()` (with `import retrofit2.create`) creates the interface like `create(Api::class.java)`. A named argument on a nested builder step and an OkHttp builder held by a local are not read, and names count only with the library's import.

Java consumers are the `RestTemplate` methods (`getForObject`, `getForEntity`, `postForObject`, `postForEntity`, `put`, `patchForObject`, `delete` and `exchange(url, HttpMethod.X, ...)`), the `.uri(...)` of a WebClient request (`.get()`, `.post()` and the others, or `.method(HttpMethod.X)`), and the `build()` of an `HttpRequest.newBuilder()` chain (`.uri(URI.create(url))`, `.GET()`, `.POST(...)`, `.PUT(...)`, `.DELETE()`, `.HEAD()` or `.method("X", ...)`) or of an OkHttp `new Request.Builder()` (`.url(url)`, `.get()`, `.post(...)` and the others or `.method("X", ...)`); a chain without a method sends `GET`. The client counts by the type the code writes (a field, a parameter, a variable), imported by name or by an `import ...*` that is the only one that could bring it, or by the call that makes it, and a WebClient's base comes from the `WebClient.create(base)` or `WebClient.builder().baseUrl(base)` it comes from, also through a constant, a `final` field with a single value or the argument each caller passes to a wrapper; for a WebClient held in a local variable only the type is read, and the base stays unknown. The URL folds literals, `+`, `static final` constants (also from other classes), `URI.create(...)`, `String.format` (each `%s` or `%d` takes the matching argument, and becomes a parameter when that cannot be read) and template variables (`{id}`). A method that builds the URL or the method from its parameters is a wrapper: each of its calls becomes a consumer with the arguments substituted, through any number of levels. `HttpMethod.X` counts through a Spring import, read in the file that writes it, also when it arrives through a wrapper; a repository type with the same name hides it. A call whose URL cannot be read is an unresolved consumer.

In Java, an `@FeignClient(name = "x", url = "...")` interface (`org.springframework.cloud.openfeign`) with methods annotated as in Spring MVC (`@GetMapping`, `@RequestMapping(method = ...)` and the others) has each method as a consumer, at the interface method itself: its callers reach the provider, and `trace` and `context` pass through it. The base comes from `url` and the prefix from `path`, both with `${key}` placeholders resolved through the Spring configuration, as in `@Value`. A `url` without a scheme gets `http://`, as in Spring Cloud. A `@RequestMapping` without `method` means `GET`, as Feign sends it. Without `url` the host is unresolved, and the link's reason carries the `name` (`host unresolved (Feign client x)`); the provider serving the path and method counts. `@RequestLine`, other Feign contracts and a `@RequestMapping` on the interface are not read. In Retrofit, an interface whose methods carry `@GET("x")`, `@POST` and the others from `retrofit2.http` has each method as a consumer; the base comes from the `Retrofit.Builder().baseUrl(base)` that creates the interface (`retrofit.create(Api.class)`), also through a constant or a wrapper, one consumer per creation, and without a creation the base is unknown. A path starting with `/` replaces the base's path (also when the base is concatenated), and a relative one follows it; an empty path is Retrofit's dynamic `@Url` and gives a consumer with no contract. A Retrofit annotation names its path in `value` alone, and a path with a `{` that does not close within its segment is not read. `@HTTP` is not read. Annotations count through an import, with the same hiding rules as Spring. An ambiguous Feign or Retrofit method (for example, without `url` and with two repositories serving the path) or an unresolved one is an open item at the method itself: `search` with `kind=ambiguous` or `kind=unresolved` lists it, and `meada decide` resolves it with the method as the excerpt.

In Rust, `base_url_env` can link a base URL read through `std::env::var`, `env!` or `dotenvy::var`. In Go, it covers `os.Getenv` and `os.LookupEnv`, including defaults with `if v == ""`, `len(v) == 0`, `!ok` or `!ok || v == ""` when `ok` comes from the same `os.LookupEnv`. It also follows `strings.TrimSuffix`, `TrimRight` and `TrimSpace`, and a struct field of the same file tagged `env:"X"` or `envconfig:"X"` (cleanenv, caarlos0/env, envconfig). A struct field that keeps the base URL (`c.base`) is the read every composite literal of the repository that sets it gives it, directly (`&Client{base: os.Getenv("X")}`) or through a constructor's parameter (`New(base string)`) whose callers in the repository all pass the same read; it applies in a method that reads the field directly or through a request wrapper. The field stays unknown when two literals or two callers give different reads, when a literal sets it by position or with a type the source does not tell (an alias), when no literal sets it, when some file writes it through a selector (`c.base = x`), when the constructor has no caller in the repository, and when a value is a literal string rather than a read.

In Java, `base_url_env` links the base URL of `System.getenv("X")`, also when a local holding the read is followed (a local the file never assigns again), and that of a field or constructor parameter with Spring's `@Value("${key}")`. The key comes from the `application.properties`, `application.yml` or `application.yaml` in the `src/main/resources` of the file's module (the module of the main source root its Maven or Gradle build declares for it, a Gradle `srcDirs` included, or else of the `src/main/` it sits under); with more than one, `.properties` wins, then `.yml`. In a `.properties` file, what follows `#---` is not read. A literal URL links by host to the peers' `[serve] hosts`. A `${VAR}` placeholder no key fills is the variable `VAR`, mapped through `base_url_env`. With a default (`${VAR:https://web.example.com/api}`), the default links by host when some peer serves that host; otherwise `VAR` applies, as without a default. Meada never reads a variable's value. A `static` field, a field some file assigns, or one a parameter or a local hides, gets no value. A constructor parameter gets Spring's value only when no call in the code creates the class, `this(..)`, `super(..)` and a `new` inside an anonymous class included; with calls, the caller's value prevails. Profiles (`application-<profile>`), relaxed binding of variables and SpEL (`#{...}`) are not read.

In Kotlin, `base_url_env` links the base URL of `System.getenv("X")` and `System.getenv()["X"]`, also through `?:` fallbacks, `!!`, parentheses and string methods such as `trim`, `trimEnd`, `removeSuffix` or `lowercase`, and that of a property or constructor property with Spring's `@Value("\${key}")` (the `$` escaped in a Kotlin string), read by the Ktor client and by Kotlin Retrofit, RestTemplate and WebClient. The configuration is the Java one (`application.properties` or `application.yml` in the module's `src/main/resources`), also in a repository with no Java file, with the same rules: a literal URL links by host, a `${VAR}` placeholder is the variable `VAR`, a default whose host a peer serves links by host, and profiles, relaxed binding and SpEL are not read. The read may stand in a property, in a `val` local to the function or in the URL expression itself; a `var` is not followed. Meada never reads a variable's value.

In C#, `base_url_env` links the base URL of `Environment.GetEnvironmentVariable("X")` (also `System.Environment`), through `??` fallbacks, `!` and `Trim`, `TrimStart` or `TrimEnd`, and that of `configuration["Section:Key"]` on a receiver declared `IConfiguration` or on `builder.Configuration` of a `WebApplicationBuilder`, read by the `HttpClient` calls and by a client's `BaseAddress`. A configuration key is the variable `Section__Key`, as .NET's environment provider names it and as the code spells the key (`base_url_env` compares it as written, though .NET ignores case). A read may be held by a local or a field. A `??` chain counts when exactly one of its operands is a read. The key is looked up in the `appsettings.json` of the project (comments, trailing commas and a byte order mark are accepted, nested objects join with `:`, and keys compare without case); a literal URL it holds is the default of that variable: when a peer's `[serve] hosts` names its host the call links by host, and otherwise through `base_url_env`. `appsettings.<Environment>.json`, `GetValue`, `GetSection`, options binding and user secrets are not read, and a read passed in from another file's wrapper is a parameter. Meada never reads a variable's value.

In Swift, `base_url_env` links the base URL of `ProcessInfo.processInfo.environment["X"]` (also `Foundation.ProcessInfo`), also through `!`, parentheses, `??` fallbacks and string methods such as `trimmingCharacters`, `replacingOccurrences`, `lowercased` or `uppercased`. A `??` counts when its fallback is a plain string literal; a second read in a fallback or in an argument, a fallback that is a call or a name, and a key that is not a literal leave the base unknown. The read may be held by a `let` (a local, a `static let` or a top-level one), by an `if let` or `guard let` of the read alone or stand in the URL itself, also inside an interpolation. A `var` assigned again, an `if var` or `guard var`, and an optional binding of anything but one read are not followed, and Vapor's `Environment.get("X")` is not a read. Meada never reads a variable's value.

Resources declared in `meada.toml` also produce contracts. With a `[serve]` table, the repository reads OpenAPI 3.0/3.1 and AsyncAPI 2.x/3.0, found by name or listed in `[serve] documents`. Routes and channels become contracts with response or payload fields. `meada status` reports what was read and ignored.

## Messaging channels

Each producer links to its consumer handlers, including within the same repository. Swift has no channel support in v1 yet.

| Channel | TypeScript / JavaScript | Python | Rust | Go | Java | Kotlin | C# |
| --- | --- | --- | --- | --- | --- | --- | --- |
| Kafka | `kafkajs` | `confluent-kafka`, `kafka-python` | `rdkafka` | `segmentio/kafka-go`, `confluent-kafka-go`, `sarama` | Spring Kafka | Spring Kafka | `Confluent.Kafka` |
| RabbitMQ | `amqplib` | `pika` | `lapin` | `amqp091-go` | Spring AMQP | Spring AMQP | `RabbitMQ.Client` |
| Amazon SQS | AWS SDK v3, `sqs-consumer` | `boto3` | `aws-sdk-sqs` | `aws-sdk-go-v2` | AWS SDK v2 | AWS SDK v2 | `AWSSDK.SQS` |
| Amazon SNS | AWS SDK v3 `PublishCommand`, `PublishBatchCommand` | `boto3` `publish`, `publish_batch` | | AWS SDK v2 | AWS SDK 2.x | | AWS SDK .NET |
| Amazon EventBridge | AWS SDK v3 `PutEventsCommand` | `boto3` `put_events` | | AWS SDK v2 | AWS SDK 2.x | | AWS SDK .NET |
| Amazon Kinesis | AWS SDK v3 `PutRecordCommand`, `PutRecordsCommand` | `boto3` `put_record`, `put_records` | | AWS SDK v2 | AWS SDK 2.x | | AWS SDK .NET |
| NATS | `nats` | `nats-py` | `async-nats` | `nats.go` | jnats | jnats | `NATS.Client` |
| Redis streams | `ioredis`, `node-redis` | `redis-py` | crate `redis` | `go-redis` | Jedis, Lettuce | Jedis, Lettuce | `StackExchange.Redis` |
| JMS | | | | | Spring JMS | Spring JMS | |
| Azure Service Bus | Azure SDK | Azure SDK | | Azure SDK | Azure SDK | | `Azure.Messaging.ServiceBus` |

The brokers are the contract ids (`channel::<broker>::<name>`): `kafka`, `amqp` (RabbitMQ), `jms`, `sqs`, `nats`, `redis`, `bullmq` and `servicebus` (Azure Service Bus). BullMQ jobs are supported too. In RabbitMQ, a `publish` to an exchange reaches queues through their bindings. NATS supports `*` and `>` wildcards. Broker clusters are not modeled: same-named topics in different clusters can link as though they belonged to one cluster.

In Java, outside the AWS forms, a client counts by the type the code writes for it (`KafkaTemplate`, `RabbitTemplate`, `JmsTemplate`, jnats `Connection` or `Dispatcher`, `Jedis`, `JedisPooled`, `UnifiedJedis`, `JedisCluster` and Lettuce's `RedisCommands`, `RedisAsyncCommands` and `RedisReactiveCommands`), and a listener annotation by its Spring import; a repository type with the same name (a class, an interface or an `@interface` annotation) hides it. Producers are `KafkaTemplate.send(topic, ...)`, `RabbitTemplate.convertAndSend(exchange, routingKey, ...)` (with two arguments, or with a `MessagePostProcessor`, it publishes to the default exchange and reaches the queue its key names, or to the configuration's `spring.rabbitmq.template.exchange`), `JmsTemplate.send` and `convertAndSend` with the destination, `sendMessage` with the request's `queueUrl` (the queue is the URL's last segment), `publish(subject, ...)` and `xadd(key, ...)`. Consumers are `@KafkaListener(topics = ...)`, also through `topicPartitions`, `@RabbitListener(queues = ...)`, `queuesToDeclare` and `bindings = @QueueBinding(...)`, whose keys become bindings (on a `direct` `@Exchange`, the default, or a `fanout` one, only keys with no wildcard; on a `headers` one, none), `@JmsListener(destination = ...)`, `receiveMessage`, `subscribe(subject)` and the streams of `xreadGroup` (Jedis, from a `Map.of`) and `xreadgroup` (Lettuce, from `StreamOffset`). `BindingBuilder.bind(new Queue(..)).to(new TopicExchange(..)).with(key)` binds a queue too. An annotation's handler is the annotated method; a polling client's is the method holding the call that names the topic or the queue, even when the receive loop runs in another. Names are evaluated like URLs: constants, `final` fields, a wrapper's arguments, a field with `@Value("${key}")` and the `${key}` placeholders of annotations, read from the module's Spring configuration. A name that does not evaluate (an environment variable, SpEL, an overload the arguments do not tell apart) is an unresolved side.

In Kotlin, the same clients, annotations and bindings count when written the Kotlin way, with Java's rules for handlers and names: named arguments and arrays (`@KafkaListener(topics = ["a", "b"])`, `arrayOf(..)`), annotations nested without `@` (`QueueBinding(value = Queue("q"), exchange = Exchange(value = "x"), key = ["k"])`), `Queue("q")` without `new` in `BindingBuilder`, `const val` at the top level or in a `companion object`, templates (`"orders-$env"` is an unresolved side while `env` does not evaluate), a client received by the constructor (`private val kafka: KafkaTemplate<String, String>`), `@Value("\${key}")` and `mapOf(key to id)` in Jedis and Lettuce streams. A call on an implicit receiver (an extension function of the client, `apply` and `with` blocks) is not read.

In C#, outside the AWS forms, a client counts by the type the code writes for it (a field, a property, a parameter or a typed local) or by the construction that made it (`new ProducerBuilder<..>(..).Build()`, `factory.CreateConnection()`, `connection.CreateModel()`, `multiplexer.GetDatabase()`, `client.CreateSender(..)`), resolved through the usings, never by name alone: a repository type with the same name hides it, and `ConnectionFactory` or `IConnection` with both `RabbitMQ.Client` and `NATS.Client` imported is neither. Producers are Confluent.Kafka's `Produce` and `ProduceAsync(topic, ...)`, `BasicPublish` and `BasicPublishAsync(exchange, routingKey, ...)` (the empty exchange, `string.Empty`, `String.Empty` with `using System;` or `""`, reaches the queue its key names), AWSSDK.SQS's `SendMessageAsync` with the queue URL or with `new SendMessageRequest { QueueUrl = .. }` (the queue is the URL's last segment), NATS.Client's `Publish(subject, ...)`, StackExchange.Redis's `StreamAdd` and `StreamAddAsync(key, ...)`, and the send methods of a `ServiceBusSender` (`SendMessageAsync`, `SendMessagesAsync`, `ScheduleMessageAsync`, `ScheduleMessagesAsync`), whose queue or topic is the argument of the `CreateSender` that made it. Consumers are `Subscribe(topic)` and `Subscribe(topics)`, `BasicConsume(queue, ...)`, `ReceiveMessageAsync`, `SubscribeAsync(subject, handler)`, `StreamReadGroup` and `StreamReadGroupAsync(key, ...)`, and `CreateProcessor(queue)` or `CreateProcessor(topic, subscription)`. `QueueBind(queue, exchange, key)` binds a queue by RabbitMQ's routing rules, and a NATS wildcard link is `inferred`. Azure Service Bus has no wildcard, and a processor on a topic consumes every message sent to the topic, whatever the subscription. A callback's handler is the callback itself (a lambda or a repository method given to `SubscribeAsync`) or each handler attached with `+=` to the `Received`, `ReceivedAsync`, `MessageHandler` or `ProcessMessageAsync` event of the consumer or processor that holds the call's result (in a local, in the same method; in a field, in any method of the class), one consumer per handler; a polling client's is the method holding the call that names the topic or the queue. `.ConfigureAwait(..)` in the chain that makes the client does not break it. Names are evaluated like URLs (constants, `static readonly` fields, locals bound once, interpolation and a wrapper's arguments at each caller, the method that makes a `ServiceBusSender` with `CreateSender(queue)` included). A `ServiceBusSender` written more than once (a field assigned in two constructors or in a setter, or a local made again) has no one name and is unresolved. A name that does not evaluate, an environment or `IConfiguration` read included (`appsettings.json` is not read here), is an unresolved side. MassTransit, `BasicGet`, `SubscribeSync` and `QueueSubscribe`, NATS.Net v2 are not read.

## npm packages

Each named `package.json` produces `package::npm::<name>`. Entries in `dependencies`, `devDependencies`, `peerDependencies` and `optionalDependencies` link to workspace packages, including those in the same repository. The range is compared with the local version: `range mismatch` when it rejects that version, `range unchecked` when it cannot be checked.

Calls to imported functions cross into provider code through `exports`, `types`, `module`, `main` and re-exports. If the entry points to build output, `tsconfig.json`'s `outDir` maps back to `rootDir`. `search` with kind `package` reports name, repository, directory, version and consumers. `context` on an exported function lists callers throughout the workspace.

## Python packages

Each `pyproject.toml` with a name under `[project]` or `[tool.poetry]` produces `package::pypi::<name>`, normalized by PEP 503. Dependencies in `[project] dependencies`, `[tool.poetry.dependencies]` and `requirements*.txt` link to workspace packages. The range stays as written and is compared with the local version as PEP 440 says: `range mismatch` when the specifiers reject it, nothing added when they accept it, `range unchecked` when the provider has no version or a specifier cannot be read. Checked are `==`, `!=`, `<`, `<=`, `>`, `>=`, `~=`, `==X.*` and `===`, joined by commas; Poetry syntax (`^1.2`, `~1.2`, `*`) and environment markers are not checked. A pre-release version is rejected unless a specifier names one. A mismatch removes no candidate.

A uv project's `[tool.uv.sources]` picks the package a `[project] dependencies` entry installs, as Cargo's sources do: `{ workspace = true }` the member of the consumer's uv workspace (the nearest `pyproject.toml` at or above it with `[tool.uv.workspace]` whose `members` globs, less `exclude`, hold it), `{ path = "..." }` the package in that directory, and `{ git = "..." }` the workspace repository of that URL, each at `exact`, for the entry and for the calls through its imports. The other packages of the name get no link. A `url` or `index` source stays out. A list of sources (one per marker), and a workspace source whose root or single member of the name is not found, keep the rule above. A package of the consumer's own repository that no uv source names (an entry in `[project] dependencies` or `requirements*.txt` with no source) is only `inferred` when it is the one package of the name, with the reason "no uv source names the local package, the registry package is the likely provider": pip and uv install the registry's package, so a same-named stub in the repository is a candidate, not a certainty. A single provider in another repository of the workspace stays `exact`.

An import reaches provider code as `exact` when its module matches the distribution name, `inferred` for one of its top-level modules and `ambiguous` when several distributions expose it. Same-named npm and PyPI packages do not link.

## Rust packages

Each `Cargo.toml` with `[package] name` produces `package::cargo::<name>`. Each Cargo workspace member is a provider. Entries in `[dependencies]`, `[dev-dependencies]` and `[build-dependencies]` resolve as follows:

- `path`: the crate in that directory, in the same repository or another workspace repository. Paths resolve from text against repository roots, without reading outside them or following symlinks. Paths outside the workspace do not appear.
- `git`: the repository identified by the normalized URL.
- Version: the same-named provider in the workspace.

`package =` gives a renamed dependency's real name. `workspace = true` reads `[workspace.dependencies]`; `version.workspace = true` reads `[workspace.package]`.

The link is `exact`, with the range as written and `range unchecked`, because Cargo ranges are not checked. A `use` follows `[lib] name` or the renamed dependency key, then `pub use` re-exports to public items. This link is also `exact`, with `; import crate name` in its reason. Dependencies outside the workspace do not appear.

## Go modules

Each `go.mod` with `module` produces `package::go::<module path>`. `require` and local-directory `replace` entries link to the workspace provider. A `replace` leaving the repository resolves from text, like Cargo's `path`; a `replace` by another module (`replace a => github.com/owner/repo`) keeps, among the providers of the required module, the one in the workspace repository whose identity is the start of that path (a `/vN` suffix dropped, a subdirectory naming the directory of the `go.mod`), at `exact`, for the entry and for the calls through its imports. A replacement no repository of the workspace holds, or one whose `go.mod` declares another module path, stays out.

The link is `exact`, with the `require` version as its range, `(local none)` as the local version and `range unchecked`. An import of `<module>/<directory>` reaches the directory's exported functions, types and methods. When the directory exists, the link is `exact`, with `; import module path` in its reason.

Meada reads neither `go.sum` nor lockfiles, including `Cargo.lock`. It queries no module proxy, npm registry, PyPI or crates.io.

## Maven and Gradle packages

Each `pom.xml` with literal coordinates and each Gradle project with a literal `group` and name provides `package::maven::<group>:<artifact>`. Maven inherits the literal `<parent>` group when needed; every module is an artifact. For Gradle, `rootProject.name` and `include` in `settings.gradle` or `settings.gradle.kts` name the projects of `build.gradle` or `build.gradle.kts` files. A project renamed or moved in `settings.gradle` (`project(":app").name`, `.projectDir`) and a `group` set in `allprojects` provide no artifact.

Maven and Gradle dependencies, map notation (`group: 'g', name: 'a'`) included, link to workspace artifacts at `exact` with `range unchecked`. A computed version (`${project.version}`, `$v`) keeps the link `exact`, with the version as written. A group or artifact using a property or variable resolves when the build files declare the value as a literal (a pom `<properties>` entry, `project.groupId` and `project.artifactId`, a parent pom of the repository, a Gradle `val`, `def` or `ext` assigned once to a plain string), and links like a written one; otherwise it stays `ambiguous`, even with one candidate, its literal fragments filtering candidates, and a coordinate with no literal fragment is no dependency. A Gradle declaration without a coordinate, such as `project(":tools")`, `files(...)`, `platform(...)` or a call, is no dependency. The repository's `gradle.properties`, the one beside `settings.gradle` and the module's (the module's wins) also fill a `${name}` or `$name` of the coordinate and the version, as long as the script does not assign the name; and a version catalog accessor (`libs.models`, `libs.my.lib` for `my-lib` or `my_lib`) becomes the coordinate that the `[libraries]` table of `gradle/libs.versions.toml` (or of the `*.versions.toml` file that a `from(files("..."))` in `settings.gradle` names by a relative path) gives it, with the version from `[versions]`. An alias the catalog lacks is no dependency. `buildSrc`, convention plugins, included builds and BOMs resolved at build time are not read. Both files are read within a size limit, in one pass. External dependencies stay out. The readers are shared with Kotlin and invoke no JDK, Maven or Gradle and query no registry.

An `import` of a class from a workspace artifact the repository depends on reaches the class in the provider through the files' `package` declaration, not a path: `context` on a public method lists the calls from other repositories that reach it, and on a public class those that reach its constructors and methods, one line per call, and `trace` crosses the hop. It covers `Type.method()` calls, `new Type(...)`, calls on a variable, field or parameter of the imported type, `import a.b.*` and static imports. The method is picked by name and argument count among the overloads of the class and of the superclasses and interfaces it has in the provider, as Java merges them. The link is `exact` when one provider declares the package and type, `ambiguous` when several do or when overloads with the same argument count remain, and `inferred` when an ancestor of the class comes from outside the provider's repository, since it may declare a better overload; the reason ends with `; import package`. Two `import a.*` imports that bring a class of the same name give no link. Source roots are `src/main/java` and `src/test/java` of each Maven module and the literal directories of Gradle `sourceSets { ... java { srcDirs = [...] } }` (`srcDir`, `srcDirs +=` and the Kotlin DSL form too). Classes under a test root, or a source set other than `main`, are never imported by a dependent; a file outside any root still resolves by its `package` declaration. Computed directories, and those inside `allprojects`, `subprojects`, `project(':x')`, `configure(...)` or a conditional block, are not read. A module of the same repository the file depends on passes on its own `compile` dependencies that are not `optional`, as Maven does; those of `test`, `provided`, `runtime` or `system` scope stay with it, and a Gradle dependency is not followed.

In Kotlin, an `import` of a top-level function, an extension function or a class from such an artifact reaches the declaration the same way: `greet("x")` picks the function of the imported package by name and argument count, `Money(5)` reaches the class, `Money.of(5)` its unnamed companion, and `order.format()` an extension function of the imported class. An extension function is followed only through the import of its name (or a star import of its package), and is `exact` when the type the source writes for the receiver is a class of the artifact, whose members are all known, and one extension of that type fits the call; on a receiver of an unknown type or of one from outside the workspace it is `inferred`, since a member of that type may hide it. A member of the receiver's type with the same name wins over it, and one the repository declares itself is not confused with it. A call that may leave a default argument out, or that fills a `vararg`, reaches every overload it could, `ambiguous` when more than one; an `internal` or `private` declaration, and one under a test root are not followed. Source roots also include `src/main/kotlin` and `src/test/kotlin`, and Gradle `sourceSets` directories are read for `kotlin { srcDirs = [...] }`, `srcDir("...")`, `sourceSets["main"]` and `sourceSets.getByName("test")` as for `java`; a Kotlin set is a test set when it is named `test` or ends in `Test`, and a main set when it is named `main` or ends in `Main`, and any other is not read. A named companion (`companion object Factory`) and `setSrcDirs` are not followed.

## NuGet packages

Each SDK-style `.csproj` provides `package::nuget::<id>`, where the id is its `PackageId`, else its `AssemblyName`, else the project file name. The `PackageId`, `AssemblyName` and version a `Directory.Build.props` writes apply to the projects below it, and the `.csproj`'s own win. A computed id (`$(Id)`), in the project or in the `props` file, provides no package. A `PackageReference` that names a workspace package links to it at `exact`, with the `Version` attribute (or element) as the range and `range unchecked`. A `ProjectReference` to a project file in another workspace repository links to that project's package at `exact`, whatever the file name says of its id; one to a project of its own repository stays inside it. A dependency outside the workspace stays out, and Meada queries no feed and reads neither `Directory.Build.targets` nor `Directory.Packages.props`. Package ids are matched without regard to case, as NuGet does; the path of a `ProjectReference` is matched in the case written.

A `using` of a namespace of a project the file depends on reaches its types by namespace: `context` on a public method or type lists the calls from other repositories that reach it, and `trace` crosses the hop. It covers `Type.Method()`, `new Type(...)`, calls on a variable, field, property or parameter of the type, even when no call of the file names the type, namespace usings and `using static`. The method is picked by name and argument count. The link is `exact` when one provider declares the namespace and type, `ambiguous` when several do, and `inferred` when an ancestor of the class comes from outside the provider's repository; the reason ends with `; import namespace`. Extension methods, a `var` local that holds a call's result and a fully qualified name written with no `using` are not followed. A file also sees what the projects of its own repository that its project references, directly or not, pass on; on a repeated name, the nearer project's reference wins. A reference whose `PrivateAssets` includes `all` or `compile`, or is computed (`$(...)`), does not pass on.

## Swift packages

Each `Package.swift` provides `package::swiftpm::<name>`, where the name is the literal of `Package(name:)`; a computed name provides the package named by its pattern, with `*` for each computed part (`Kit*`). The `.library(name:targets:)` products and the targets of each are recorded, and only the modules of a library product can be imported. A `.package(url:)` links to the workspace repository whose remote identity (ADR 0004) is the URL's, and a `.package(path:)` to the repository at that path, `exact`, with the written requirement (`from`, `exact`, `branch`, `revision`, `.upToNextMajor`, ranges) as the range, `(local none)` and `range unchecked`. Only literal values count: a computed URL, path or name (`"\(vendor)"`) makes the link `ambiguous`, listing every provider whose literal parts fit; so does a package whose own `Package(name:)` is computed. A dependency outside the workspace is left out, and Meada reads no `Package.resolved` and asks no network.

An `import Module` naming a module of a package the `Package.swift` depends on reaches its exported declarations: `Module.f(a:)`, `f(a:)`, `Type(a:)` and `value.method(b:)` on a variable, parameter or local constant whose type is written or that was just constructed. The argument labels pick among the overloads, and a parameter with a default value may be left out. Only `public` or `open` declarations are followed. The link is `exact` when one package exposes the module and `ambiguous` when several packages the file depends on expose the module, even if only one declares the name; the reason ends with `; import module`. Platform modules (`Foundation`, `Dispatch`, `SwiftUI` and others) never make a link. All the dependencies of the nearest `Package.swift` count for the files below it, without telling apart what each target declares with `.product(name:package:)`, and an `internal` member of a public type, an inherited member and an extension from another module are not told apart.

## Shape drift

Drift is a field a `fetch` consumer reads from a response but the route does not return. Meada compares the read with OpenAPI, a zod schema or the handler's object literals.

| Query | Result |
| --- | --- |
| `trace` | Adds `drift: missing <path>` to the link |
| `context` on a route | Shows shape origin, consumers, drift, level and unchecked reads under open parts |
| `search` with kind `drift` | Lists detected drift |
| `meada status` | Counts missing reads in the consumer repository; the provider receives no count |

## Config values

Every scalar of every `*.yaml`, `*.yml` and `*.json` file Meada reads, and of the YAML and JSON files under the CI configuration at the root (`.github/`, `.gitlab-ci.yml`, `.gitlab/`, `.circleci/`, `.buildkite/`, `.drone.yml`), is a config value: file, key path (jq style, `alerts.loki[0].labels.support`, `metadata.labels["app.kubernetes.io/name"]`, `[2].spec.replicas` in a file of several documents), value, type (string, number or boolean) and line. Other hidden paths stay out, and nothing needs turning on.

Terraform and OpenTofu files are config values too (`*.tf`, `*.tofu`, `*.tf.json`, `*.tofu.json`, `*.tfvars`, `*.auto.tfvars`, `*.tfvars.json`). The key path runs over the block's type and labels and the attribute's name (`resource.aws_sqs_queue.orders.name`, `resource.opsgenie_team_routing_rule.critical.criteria.conditions[1].expected_value`), a block repeated under the same type and labels indexed `[n]` (`ingress[0]`). A literal keeps its type; an expression (`var.environment`, `format(..)`) or a template (`"${var.environment}-orders"`, a heredoc with an interpolation) is stored as written, under the type `expression` or `template`, until static evaluation resolves it. A `.tofu` file shadows the `.tf` file of the same name in its directory (`.tofu.json` the `.tf.json`), as OpenTofu does, and the shadowed file is not read. Beside an override file it counts as one that does not read, since Terraform, which ignores `.tofu` files, reads it.

| Query | Result |
| --- | --- |
| `search` with kind `value` | Values whose value or key path equals, starts with or holds the query, as `<repository> <path>:<line> <key path> = <value>` |
| `search` with no kind | Values only when no symbol and no contract matches |
| `meada status` | Config files read and skipped (lockfiles, files that do not parse) and the values redacted |

A value over 1 KiB is stored cut, marked `(cut)`. The `data` and `stringData` of a Kubernetes `Secret` are stored as `redacted`, with key path, type and line, as are the `default` of a Terraform `variable` with `sensitive = true` (or `"true"`, which Terraform converts), also when another file of its directory marks it so and the directory holds an override file (`override.tf`, `*_override.tf`), which Terraform merges into the others, and every value a `.tfvars` file gives a variable of that name (all of them, when a Terraform file at or above it does not parse) and the `value` of an `aws_ssm_parameter` resource with literal `type = "SecureString"`, in HCL or Terraform JSON; nothing else is filtered, so files that must stay out of the index go in `.meadaignore`. Lockfiles (`package-lock.json`, `npm-shrinkwrap.json`, `pnpm-lock.yaml`, `packages.lock.json`), Helm templates (not YAML until rendered), files over 1 MiB and files of more than 1,000 documents are skipped. Value text never enters `meada export`.

An AWS ARN recognized by the type table, an SQS URL or `s3://archive/path` in config becomes an `exact` use side, located by `<file>#<key path>`. A plain name under the final key `queue`, `topic`, `bucket` or `table` becomes an `inferred` SQS, SNS, S3 or DynamoDB use, respectively. For example, `queue: orders` in Helm values uses `channel::sqs::orders`; `other: orders` links nothing. A use has no direction: it links only to provisioners and counts for orphans, without becoming a producer, consumer, writer or reader. `context` and contract search show these uses, and `trace` follows provisioning links to the key and back. Redacted, cut or unresolved values stay out; the attribute defining a provisioner's name does not use its own resource. Export includes the sides in `uses`, without value text, and the console shows them separately from directed sides.

## IaC nodes

Every `resource`, `data` block and `module` call of the `*.tf`, `*.tf.json`, `*.tofu` and `*.tofu.json` files is an IaC node, with an address (`<type>.<name>`, `data.<type>.<name>`, `module.<name>`), a type, a file, its lines and the root module its evaluation starts from: a directory no other one calls as a local module, labelled by its path (`.` at the root). Each call of a local module (a `./` or `../` `source` that stays inside the repository) instantiates its blocks with the call's address in front (`module.orders.aws_sqs_queue.this`), and a `count` or a `for_each` over values that resolve to literals gives one node per instance, with `[key]` at the end (`aws_sqs_queue.dlq[0]`). An expression that names a resource, a data block or a module is a reference, stored like a call; one that goes through `var.<name>`, `local.<name>` or a module's output is followed to what it names, and keeps the name it went through.

Terraform JSON uses the same evaluation and reference rules. Ordinary strings are templates; a sole interpolation keeps its result type, so `${var.items}` can supply a collection. Variable defaults and module sources are literal. Static `depends_on` strings name references. Block arrays retain their indexes in config key paths. Both JSON extensions support node addresses in declared resource `via` locations.

| Query | Result |
| --- | --- |
| `search` with kind `iac` | Nodes whose address, type or physical name (the `name` argument as static evaluation resolves it) equals, starts with or holds the query, as `<repository> <path>:<line> <address> (<type>, name "<name>", root <root module>)`, `pattern` in place of `name` when a part is unresolved |
| `search` with no kind | No IaC node: only the config values that hold its name, when nothing else matches |
| `context` on an address | Type, physical name (`(pattern)` when a part is unresolved), root module, the root variables the name went through (`overridable:`, `(default)` or `(tfvars)`), the references both ways (`references:`, `referenced by:`, each with its `via`) and the block's config values |
| `trace` from an address | The references, `down` (what the node reaches) or `up` (what reaches it), within the root module and through the local modules |

Every reference is `exact`: a name as written, followed without evaluating values. Static evaluation
resolves each node's physical name and each `count` and `for_each`, without running Terraform or
reaching the network, from the repository's own values: variable defaults, the root's
`terraform.tfvars`, `terraform.tfvars.json`, `*.auto.tfvars` and `*.auto.tfvars.json` (in that
order, a later file winning), locals, a local module's inputs and outputs per call, `each`,
`count.index`, templates and the pure string, collection and conversion functions of ADR 0025 with
`jsonencode`. Override files (`override.tf`, `*_override.tf`) merge into the blocks of their
directory after its other files, as Terraform applies them: what an override block sets replaces
the base block's, references included, and beside one, a Terraform file that does not read
leaves no variable of the directory evaluated, as it may mark any of them sensitive. Data
sources, resource attributes, registry modules,
`terraform.workspace`, `-var`,
`-var-file`, `TF_VAR_`, sensitive variables and any other function are never evaluated: such a part
becomes a `*` and the name a pattern (`orders-*`), matching literal names under the pattern rules below. A name that went through a
root module's variable, by its default or a var file, records it, since a deployer can override it.
A value over 64 KiB, an expression whose evaluation could make over 1 MiB and what a root evaluates
past 64 MiB stay unresolved. A changed Terraform file or var file evaluates again only the root
modules whose unit (the root and the local modules it calls) holds it. A `count` or `for_each` that
does not resolve gives one node with no key; a registry or remote module is only the call's node.
Only nodes holding a contract side enter `meada
export`.

### Orphan contracts

Default `search`, and `search` with `kind = "contract"`, `channel` or `resource`, also find provisioner sides by contract.

`meada status` lists orphan contracts per kind and broker (or resource kind): provisioners
with no use and uses with no provisioner. Orphan lines appear only when the queried repository
or workspace contains at least one provisioner and one use of that group. A code-only or
IaC-only repository produces no such lines; `search` and `context` mark the side itself with
`no provisioner in the workspace` or `no use in the workspace`. Provisioning links, including
patterns and independent root modules, determine whether a counterpart exists. Existing
warnings about declared resources without writers and readers remain separate.

A use Meada only infers from a key name (a plain value under `queue`, `topic`, `bucket`, `table`
or a typed alias, `inferred`) opens no orphan scope for its group and is never an orphan line: a
name match proves neither a use nor its absence. It still links to the provisioner of the same
name, which is therefore no `no use`, and `context` still shows it with `[inferred]`. A
`no provisioner` line ends with its side's level in brackets, as `context` writes it (`[exact]`,
`[declared]`), and the `uses` rows of `meada export` carry `level`.

A structured identifier in the resource list of an IAM policy statement (`Resource` or
`NotResource` of an entry of `Statement`, in a policy file or in a policy document inside a
template, and `resources` or `not_resources` of a `statement` block of an
`aws_iam_policy_document`) is a **policy grant**, not a use: it says who may touch the resource,
with no direction. `context` on the resource lists it with the role `policy grant` and its place;
it links to no provisioner, opens no orphan scope and is never an orphan line, and `meada export`
lists it in the `policy_grants` array, apart from `uses`. In a JSON Schema document (one with
`$schema` at its root), a value under `examples`, `default`, `enum` or `const` documents a shape and
is no side at all: it stays a config value that `search` finds.

In a document that the reader of a contract kind recognizes as its own, Meada infers no use from a
key name: a Prometheus or Loki rule file (`groups` holding `rules`), a Grafana alert rule file, an
Alertmanager configuration, a `PrometheusRule` or `AlertmanagerConfig` object (that document of the
stream only) and a provider document (an OpenAPI or AsyncAPI document that Meada reads, in a
repository with `[serve]`). There, a rule's `queue: jobs` label is an alert label, not the use of a
queue; a structured identifier (an ARN) is still an `exact` use, and the values stay config values
that `search` finds. IaC files (Terraform, OpenTofu, CloudFormation, SAM, Serverless, Compose and
the other documents of a Kubernetes stream) still give such uses, and the same label in a file no
reader recognizes is still an `inferred` use.

### Terraform provisioners

An `aws_sqs_queue` resource provisions `channel::sqs::<name>` through its `name` argument;
an `aws_s3_bucket` provisions `resource::s3::<name>` through `bucket`. The type table in
`src/iac/types.toml`, embedded in the binary, also reads names from ARNs and URLs. A declared
`[[resource]]` with `kind = "s3"` and the same `id` joins that contract, keeping `declared`.

CloudFormation and SAM use the same contracts for SQS, SNS, EventBridge, Kinesis, S3,
DynamoDB (Table, GlobalTable and SimpleTable), RDS (instance or cluster), ElastiCache
(cluster or group), SSM and Secrets Manager when the physical name is known. The property
defining that name does not use its own resource; secrets contribute only their identity.

Every producer, consumer, writer and reader links to each provisioner, identified by address
and root module. Roots such as `envs/dev` and `envs/prod` are never ambiguous among themselves.
Written names give `exact`; names passing through overridable values give `inferred`, with the
variables and their sources in the reason. `context` on the block lists its contract sides;
on a producer it lists provisioners. `trace` follows the block to code with `down` and code to
infrastructure with `up`. Export keeps `format: 1` and adds `iac_nodes`, `provisioners` and
`provisions`, omitted when empty. Unlisted types and data blocks do not provision contracts. A
pattern
with at least three literal letters or digits (separators do not count) and wildcards on segment
boundaries (`-`, `_`, `.`, `/`) links to a literal name at `inferred`, or `ambiguous` when several
names match. Patterns never link to patterns. The console graph lists provisioner sides and their
separate links.

### AWS SDKs in Java, Go and C#

The operations below create contract sides without a `meada.toml` declaration. The client must come from an SDK constructor, with every hop used to establish that origin resolving through an exact call. Parameters, locals and type names that hide the client or its constructor do not inherit its origin. SQS keeps its earlier forms in Java, Go and C#: a field, parameter or typed local the source declares with the SDK's client type (`SqsClient` or `SqsAsyncClient`, `*sqs.Client`, `IAmazonSQS` or `AmazonSQSClient`) is a client, injected ones included; a repository type of that name is not. A literal name joins its provisioned physical name; an environment read without a matching workload entry stays unresolved and retains `env:NAME` in the export, including producers. Meada never reads the process environment.

Environment reads through Go and C# import aliases retain their variable name. A named request stays unresolved when its name field is written, including through an alias.

| Service | Operations and direction |
| --- | --- |
| SQS | `SendMessage`, `SendMessageBatch` produce; `ReceiveMessage` consumes, through `QueueUrl` |
| SNS | `Publish`, `PublishBatch` produce, through `TopicArn`; a phone or endpoint publish names no topic |
| EventBridge | `PutEvents` produces to every `Entries[].EventBusName`, `default` when omitted |
| Kinesis | `PutRecord`, `PutRecords` produce, through `StreamName` |
| S3 | `PutObject`, `DeleteObject` write; `GetObject`, `HeadObject`, `ListObjects`, `ListObjectsV2` read, through the bucket |
| DynamoDB | `PutItem`, `UpdateItem`, `DeleteItem`, `BatchWriteItem`, and the `Put`, `Update` and `Delete` entries of `TransactWriteItems` write; `GetItem`, `Query`, `Scan`, `BatchGetItem`, the `Get` entries of `TransactGetItems` and the `ConditionCheck` entries of `TransactWriteItems` read, through `TableName`, every `RequestItems` key or the `TableName` of every `TransactItems` entry |
| SSM | `GetParameter`, `GetParameters` read; `PutParameter`, `DeleteParameter`, `DeleteParameters` write, through `Name` or every `Names` entry |
| Secrets Manager | `GetSecretValue`, `BatchGetSecretValue` read; `PutSecretValue`, `UpdateSecret`, `CreateSecret`, `DeleteSecret` write, through `SecretId`, every `SecretIdList` entry or `Name` |

Operation names use Go spelling; Java starts lowercase and .NET accepts synchronous and `Async` forms. .NET S3 uses `BucketName` and `GetObjectMetadata` for `HeadObject`. ARNs reduce to the infrastructure's physical name. `GetParametersByPath` stays unresolved because `Path` is a hierarchy, not a parameter. RDS and ElastiCache control-plane operations do not establish data reads or writes. An unnamed resource never links; a `meada.toml` declaration retains `declared` level.

### GCP and Cloud Run

Terraform/OpenTofu recognizes `google_pubsub_topic` and `google_pubsub_subscription` as
`pubsub` channels, `google_storage_bucket` as `gcs`, `google_sql_database_instance` and
`google_sql_database` as `cloudsql`, and `google_secret_manager_secret` as
`gcpsecretmanager`. The name comes from `name`, except secrets use `secret_id`.
Cloud SQL instances use `<instance>`; databases use `<instance>/<database>`, with a literal
instance or a resolved reference to a known instance. A database does not provision its
instance. Projects stay outside the identity.

A topic uses `channel::pubsub::<id>`; a subscription uses
`channel::pubsub::subscriptions/<id>`, including when both share the same id. A subscription
links its target's consumers to producers of the topic named by `topic`: `exact` without a
filter, `inferred` with a filter or overridable names. An unresolved source adds no route
and never invents a topic from the subscription's name.

`gs://<bucket>/object`, `projects/<project>/topics/<id>`,
`projects/<project>/subscriptions/<id>` and `projects/<project>/secrets/<id>` (also with
`/versions/<version>`) are `exact` config uses. Plain names under final keys
`pubsub_topic`, `pubsub_subscription`, `gcs_bucket`, `cloudsql_instance`, `cloudsql_database`
and `gcpsecretmanager_secret` are `inferred`; `cloudsql_database` requires
`<instance>/<database>`. Generic `topic` and `bucket` retain their SNS and S3 meanings.
Malformed identifiers, hidden or cut values and bare text equality create no use.

`google_cloud_run_service`, `google_cloud_run_v2_service` and `google_cloud_run_v2_job` are
workloads: each container keeps its image and environment. Images join the repository that
publishes them; overridable evidence weakens the link. Environment reads receive values only
from the container running their code and remain `inferred`. Secret Manager references stay
unresolved targets, without reading secret contents. Search, `context`, `trace` in both
directions, scoped orphans and export include the new contracts and runners; environment
entries and configuration values stay outside the export.

### GCP SDK clients

The five GCP SDK groups (JavaScript/TypeScript, Python, Java, Go and C#) recognize the documented Pub/Sub publish and subscription receive, Cloud Storage read/write and Secret Manager access forms. The [user guide](https://github.com/ClaudioMascaro/meada/blob/main/docs/user-guide.en.md#gcp-sdk-use-in-five-language-groups) lists the exact operations and constructors. Topic/subscription handles retain distinct identities, including workload environment resolution; an environment read alone stays unresolved.

### AWS channels, data, secrets and subscriptions

The Terraform/OpenTofu table also recognizes the types below. ARNs reduce to physical names,
including parameter and secret paths; the six-character Secrets Manager ARN suffix is removed.
The same provisioning, `context`, `trace` and export rules apply to every contract.

| Terraform type | Physical name | Contract |
| --- | --- | --- |
| `aws_sns_topic` | `name` | `channel::sns` |
| `aws_cloudwatch_event_bus` | `name` | `channel::eventbridge` |
| `aws_kinesis_stream` | `name` | `channel::kinesis` |
| `aws_dynamodb_table` | `name` | `resource::dynamodb` |
| `aws_db_instance` | `identifier` | `resource::rds` |
| `aws_rds_cluster` | `cluster_identifier` | `resource::rds` |
| `aws_elasticache_cluster` | `cluster_id` | `resource::elasticache` |
| `aws_elasticache_replication_group` | `replication_group_id` | `resource::elasticache` |
| `aws_ssm_parameter` | `name`, leading `/` kept | `resource::ssm` |
| `aws_secretsmanager_secret` | `name` | `resource::secretsmanager` |

An SSM `SecureString` parameter stores `value` as `redacted` in HCL and Terraform/OpenTofu JSON. When `type` is an expression the
value reader cannot resolve, its value is also hidden conservatively.

An `aws_sns_topic_subscription` with `protocol = "sqs"` links queue consumers to topic
producers. `aws_cloudwatch_event_rule` with `aws_cloudwatch_event_target` does the same from a
bus to a queue; absent `event_bus_name` means `default`. Without a filter the level is `exact`;
with `filter_policy` or `event_pattern` it is `inferred`. Overridable names also give `inferred`,
with their variables in the reason. The reason identifies the subscription or target and rule.
Unresolved endpoints, other protocols and targets without a unique rule in the same bus add no
links. Timer-only rules do not route events published to the bus.

`context` lists producers and consumers through the subscription. `trace` goes up from the
consumer to the producer and down from the provisioner to consumers; an `sns`, `eventbridge`
or `kinesis` source channel can also start a trace. Subscription summaries enter the optional `subscriptions` array with source, target, node and
root. Their links enter `links`, keeping
provisioning links separate in `provisions`. `meada status` lists provisioned contracts without
use and uses without a provisioner, per kind and broker, only when the workspace contains at
least one provisioner and one use of that group. Other IaC formats remain nodes and references;
this contract table covers `.tf`, `.tf.json`, `.tofu` and `.tofu.json` files.

## Kubernetes and Kustomize

Every YAML or JSON document with an `apiVersion`, a `kind` and a `metadata.name` is an IaC node, addressed `<Kind>[.<group>]/<namespace>/<name>` (`Deployment.apps/shop/api`, `Service/shop/api`; the core group writes no group). A cluster-scoped kind has no namespace in its address (`ClusterRole.rbac.authorization.k8s.io/reader`), and neither has an object that sets none and that no overlay gives one. A Helm template (a name with `{{`) and a Kustomize file are no object.

A directory with a `kustomization.yaml` that no other kustomization names as a base is an overlay and an evaluation unit. Its `resources`, `bases` and `components` that name a file or another kustomization inside the repository are instantiated under it, each node labelled by the overlay (`root overlays/dev`), with the `namePrefix`, `nameSuffix`, `namespace` and `images` of every kustomization on the way applied, the base's first and the overlay's last: one base used by two overlays is one node in each, under its own name and namespace. A `..` that stays inside the repository is followed (`../../base`). An entry that is remote (a URL, `github.com/org/repo//path?ref=v1`), above the repository root, absolute, missing, reached through a symlink, in a cycle or nested past 16 levels is not followed, never fetched, and `meada status` counts it (`kustomize entries not followed: N`). The manifests no kustomization lists, or patches with, are plain: the manifests of one directory are an evaluation unit, labelled by the directory.

| Reference | Stored as |
| --- | --- |
| A Service to the pod template owners (Deployment, StatefulSet, DaemonSet, Job, CronJob, Pod) whose pod labels hold its selector | `via selector app=api` |
| A workload to the ConfigMaps and Secrets of its `env`, `envFrom` and volumes | `via env DATABASE_URL`, `via envFrom`, `via volume settings` |
| A workload to its `serviceAccountName` | `via serviceAccountName` |
| An Ingress to the Services of its rules and default backend | `via backend shop.example.com/api`, `via defaultBackend` |

A reference stays in one namespace and one unit, and names an object as it was written, before the overlay's prefix and suffix, which the overlay then applies to both sides. Every reference is `exact`. `search` with kind `iac` and `context` answer for these nodes as for Terraform's, and `context` on a workload adds `images:`, what each container runs once the overlay's `images` applied (`web: registry.example.com/shop/api:1.4.0`). Patches, generators, `commonLabels`, Helm templates and a `kind: List` are not applied or read, and objects stay out of `meada export`.

### Crossplane

Crossplane objects are Kubernetes objects, and `context` says what each is to Crossplane on a `role:` line: a composite resource definition (XRD), a composition (both with no namespace), a claim or a composite resource (an object of a claim or composite kind an XRD of the repository defines), a managed resource (an object with `spec.forProvider`) or a resource a composition creates. Each resource a composition creates from a literal `base`, in `spec.resources` or in the `input.resources` of a pipeline step that runs function patch-and-transform, is a node `<composition>/<resource>` of the base's kind, named by its `crossplane.io/external-name` annotation or its `metadata.name`; a managed resource's name is its external name too.

| Reference | Stored as |
| --- | --- |
| A claim to the XRD of its composite kind | `via composite XPostgreSQLInstance.database.acme.io` |
| A composite resource to its XRD | `via kind XNetwork.network.acme.io` |
| A claim or composite resource to the composition its `compositionRef` names | `via compositionRef` |
| An XRD to its default and enforced composition | `via defaultCompositionRef`, `via enforcedCompositionRef` |
| A composition to the XRD of its `compositeTypeRef` | `via compositeTypeRef XPostgreSQLInstance.database.acme.io` |
| A composition to each resource it creates | `via resources`, `via pipeline <step>` |
| A created or managed resource to the definition of its kind (an XRD or a `CustomResourceDefinition`) | `via kind Bucket.s3.aws.upbound.io` |

Every reference is `exact` and stays in one unit, as Kubernetes ones do: a claim in another directory than its XRD keeps its role and references nothing. `trace` from a claim walks down to its XRD, the composition and the resources it creates. Patches and functions are not evaluated: a name a patch (or a patch set) sets, or a base with no name, is the pattern `*`, and a step that runs any other function (Go templating, KCL) creates no node.

### Compose

A Compose file (`compose.yaml`, `compose.yml`, `docker-compose.yaml` or `docker-compose.yml`) with its override file in the same directory is one evaluation unit, rooted at the directory. Each service is an IaC node `services.<name>` of type `service`. `depends_on`, `links` and shared networks (`networks:`; a service with none is on `default`) are references, with `via depends_on`, `via links` or `via networks.<name>`. `${VAR:-default}` resolves to its default and `context` marks it `(overridable)`; `${VAR}` stays in the text as an unresolved part, marked `(unresolved)`. `context` shows the service's `image:` and `build:`, recorded for the image contract, with the Dockerfile the build names (`dockerfile` or `dockerfile_inline`); a build context is kept only as a path inside the repository (an absolute path, a URL, one that climbs out of the root or one with an unresolved part is not).

### CloudFormation and SAM

A qualifying template named `compose.yaml` keeps its nodes and references through cold indexing and warm refreshes; its filename does not make it Compose.

A YAML template (the short-form tags `!Ref`, `!Sub`, `!GetAtt`, `!Join` and the rest are read) or a JSON one with a `Resources` mapping that carries `AWSTemplateFormatVersion` or the SAM transform (`AWS::Serverless-2016-10-31`) is an evaluation unit, rooted at its path. Each resource is an IaC node addressed by its logical id within the template, its type (`AWS::SQS::Queue`, `AWS::Serverless::Function`: SAM types are nodes like any other) the node's type. `Ref`, `Fn::GetAtt` (with the attribute in the `via`: `via Fn::GetAtt.Arn`), the variables of `Fn::Sub` and `DependsOn` are references, every one `exact`. The physical name of a queue, topic, bucket, table, function, stream, secret and a few more types is evaluated within the template: a parameter's default (`context` lists it under `overridable:`), `Ref`, `Fn::Sub`, `Fn::GetAtt` and `Fn::Join`. A pseudo parameter (`AWS::Region`), an import (`Fn::ImportValue`), any other function and a parameter with no default stay unresolved parts: the name is a pattern, with a `*` for each. A parameter with `NoEcho` has its default stored as `redacted` among the config values and no name goes through it. A changed template is evaluated again alone. Nested stacks, macros and conditions are not followed.

### Serverless Framework

A `serverless.yml`, `serverless.yaml` or `serverless.json` with a `service` is an evaluation unit, rooted at its path. Each function is an IaC node `functions.<name>` of type `Serverless::Function`, named by its `name` or `<service>-<stage>-<function>` (the stage overridable by `--stage`, `opt:stage`), with its handler and its events recorded and shown by `context` (`handler:`, `event:`: `http GET /orders`, `sqs Queue.Arn`, `schedule rate(5 minutes)`). A function references, `exact`, the resources its body names with `Ref`, `Fn::GetAtt` or `Fn::Sub`. The `resources` section is read as CloudFormation: each resource is a node at its logical id, with the names and references described above. `${self:...}` resolves to the value at that path of the file, and the default of `${opt:...}` resolves and is marked `(overridable)`, since a deployer passes it on the command line; `${env:...}`, `${ssm:...}`, `${sls:...}`, `${file(...)}` and the other sources stay unresolved parts: a `*` in a name, `${...}` in a recorded text. A value that names itself, directly or around a cycle, is unresolved, and `${self:provider.stage}` with no stage in the file is `dev`, as in a function's name. A changed service is evaluated again alone. `serverless.ts`, plugins and values from other files are not read.

### GitHub Actions

A `.yml` or `.yaml` file directly under `.github/workflows/` that holds `jobs` is a pipeline, and an `action.yml` or `action.yaml` that holds `runs` is an action, in any folder; with both names in one folder the action is `action.yml`, the file the runner loads. Each file is one evaluation unit, rooted at its path, and only its first YAML document is read. The file is an IaC node at the address `.` (type `workflow` or `action`), which a query names by the file's path. Each CI job is a node `jobs.<id>` of type `job` (`jobs["publish-prd"]` when the id is not a plain identifier), and each CI step is a node `jobs.<id>.steps[<n>]` or `runs.steps[<n>]` of type `step`. A CI job with a `strategy.matrix` is one node. References stay inside the file: the pipeline references its CI jobs, a CI job or a composite action references its CI steps, and a CI job references the CI jobs its `needs` names (`via needs`).

`search` with kind `ci` finds these nodes by address, type, `name`, a CI job's id and a CI step's `id` or `uses`, as `<repository> <path>:<line> <address> (<type>, id "<id>", name "<name>", uses "<uses>")`; kind `iac` and the default search do not show them. `context` shows the node's lines, its references both ways and the config values of its block, such as a CI step's `with` and `env`. A CI step is also named by a selector in place of its index (`jobs.build.steps[id=login]`, and `name=` and `uses=` alike). A selector is never identity: a CI step's address is positional, and a CI step inserted before it renumbers it.

A CI job whose `uses` names a reusable pipeline of another repository of the workspace (`<owner>/<repo>/.github/workflows/<file>@<ref>`) is a consumer of the CI contract `ci::<repository identity>/<path>`, and the file node of that pipeline, one whose `on` holds `workflow_call` (as a string, a list item or a key), is its provider. A `uses` writes no host, so the host is the consumer's own; owner and repository are lower-cased and the path stays as written. The link is `exact`, and its reason shows the Git ref as written and the commit the provider's index was read at, `@main (indexed 1a2b3c4)`: Meada never resolves a ref, and adds `, ref mismatch` only when the ref is a full commit id (40 or 64 hexadecimal digits) other than that commit. One contract holds every call to a file, whatever refs they name. `context` on the pipeline lists each CI job that calls it, with its repository, file, address, Git ref and the keys of `with` and `secrets` it passes (`secrets: inherit` as that word), and `context` on a CI job shows the pipeline it calls. `meada export` carries the sides, the links and, on a consumer, the optional fields `ref`, `with`, `secrets` and `permissions`. `search` with kind `contract` lists these contracts, and the default search answers as before. A `uses` whose repository is not in the workspace (`actions/checkout`, a pipeline of another organization) is external: it holds no side, so it is never an orphan or an open item. A matrix is one consumer, and a call from one host to another has no link.

Each calling CI job also shows the `permissions` block that governs it: its own, else its pipeline's, else `not declared` (#834). A block is closed, so a permission it does not name is `none`, and `read-all`, `write-all` and `{}` read as the blocks they stand for. Under `provides`, a `permissions:` line says what the CI jobs of the called pipeline declare together, the highest level for each permission, and when that is above what the caller's block holds the link's reason ends with `permissions mismatch` and names the permission and both levels (`id-token write against none`). A side that declares nothing is never compared, and a CI step that calls an action carries no permissions text. These are the permissions the files declare at the indexed commit, not the effective ones of a run: Meada reads no default of a repository or of an organization.

The other forms of `uses` read as GitHub resolves them. A CI step whose `uses` names an action of the workspace (`<owner>/<repo>[/<folder>]@<ref>`) is a consumer of that action's CI contract: `ci::<repository identity>/<folder>`, or `ci::<repository identity>` alone for an action at the root, provided by the action's file node. A local call of a CI job (`./.github/workflows/<file>`) and a `$/<path>` of a CI job or a CI step name the repository of the file that writes them: they link `exact` with the reason `own repository` and no ref, also in a repository without `origin`. A CI step's `./<folder>` is read in the CI runner's workspace, so it links to its own repository only in a pipeline that is not reusable, after an `actions/checkout` of the same CI job that names neither `repository` nor `path`. In a reusable pipeline, in a composite action, with no checkout before it or after a checkout that names one of the two, it is unresolved: an open item with no candidate, which `search` with kind `unresolved` and `context` on the CI step show with the cause. `docker://<image>` holds no CI contract, and a local path that leaves the repository root holds no side. `context` on an action lists every CI step that calls it, with its Git ref and the keys of `with`.

`meada status` lists a reusable pipeline or an action that nothing in the visible scope calls as an orphan, with its contract and its file, only when that scope (the repository alone, or the workspace) holds at least one provider and one consumer of the CI contract. A repository of actions and reusable pipelines indexed alone prints no orphan line, and neither an external `uses` nor an unresolved CI step opens the scope. A call whose repository is in the workspace and whose path names no provider there is marked on the consumer's side, ` (absent at the indexed commit)`: in `context` on the CI job or the CI step, in `context` on the contract identifier and in `search` with kind `contract`, and as `"absent": true` on the consumer's row of `meada export`. Absent is not broken: the file may exist at the Git ref the call names, which Meada does not resolve, so the mark is neither an orphan line nor an open item. A pipeline file that is there and holds no `on.workflow_call` gets ` (absent at the indexed commit: the file is no reusable pipeline there)`. A folder that holds a `Dockerfile` and no action file is an action for GitHub that Meada does not read: a call to it holds no side, so it carries no mark.

In a pipeline or an action the CI reader decides what a key means. A plain name under a typed final key (`queue`, `topic`, `bucket`, `table` and the Azure and GCP aliases) gives no use there: `with: { bucket: x }` names an input of the action, not an S3 bucket. A structured identifier in a value (an ARN, a queue URL, `s3://`, `gs://`) stays an `exact` use, and in `meada export` its `site` is the CI step that holds the key, the CI job when no CI step holds it and the file otherwise, with the same node as the `side` of its provisioning link. A role ARN in `with` gives no use. A `${{ env.X }}` resolves where a literal `env` of the CI step, of its CI job or of the pipeline gives `X`, the nearest first, so `s3://${{ env.BUCKET }}/x` links as the literal does. The nearest `env` that defines `X` decides: when its value there is no literal string (a number, a boolean, a list), the part stays unresolved and is never read from an `env` farther up. Any other expression is an unresolved part: inside a name of an AWS form (an ARN, a queue URL, `s3://`) it makes a pattern (`s3://acme-${{ vars.STAGE }}-releases/x` uses `resource::s3::acme-*-releases`, which links `inferred` to the one bucket that fits); inside a `gs://` name, a Pub/Sub path or an Azure URL, and as the whole name, it gives no use. `secrets.X` and `vars.X` have no value in a repository: they stay as written in the config value and are never matched by name across repositories. Every scalar stays a config value and `[[resource]]` with `via` on a pipeline key stays valid. A file under `.github/` that is no pipeline, an `action.yml` with no `runs` and the files of other CI systems keep the key-name rule.

The `run` script of a CI step is read through the command table, with no shell evaluated. It reads a script under `shell: bash`, `sh` or no declared shell (the CI step's, else the `defaults.run.shell` of the CI job, else the pipeline's), of at most 1024 bytes and with no heredoc. A command is the first word of a line (a trailing `\` joins two) or the one after `&&` or `;`, also inside `if` and loops; text in `echo`, in a string or in a comment gives no side, and what follows `||` or a pipe is not read up to the end of its line. `pip install`, `pip3 install`, `python -m pip install`, `uv pip install`, `pipx install` and `uv tool install` make the CI step a consumer of `package::pypi::<name>` for each name a repository of the workspace provides, with the range as written in the link's reason: `context` on the package lists the CI step among its dependents, and `context` on the CI step shows `consumes package::pypi::<name>` and the provider. The level is the package kind's rule: `exact` for a single provider in another repository, `inferred` when it sits in the CI step's own repository. A path, a URL, `-e`, the file of `-r` (not followed) and the value of any option give no side. A `${{ env.X }}`, a `$X` or a `${X}` is what a literal `env` of the CI step, the CI job or the pipeline gives; any other expression or variable (`secrets.X`, `vars.X`, `github.*`, `matrix.*`, `$(...)`, backticks) makes, in part of a name, a pattern (`inferred` to the one package it fits, `ambiguous` to several) and, as the whole name, in a workspace that provides a PyPI package, an unresolved consumer that shows what is written: an open item with its cause and no candidate. In the range or in an extra it changes nothing: the name is literal and the range stays as written. Limits: a variable the script assigns, `cd`, a function and a called script (`./deploy.sh`, `make`) are not followed; a Windows CI runner defaults to `pwsh` and the script is read as `bash`; a name the workspace provides wins over a public package of the same name.

A CI step whose command is a console script of a PyPI package of the workspace starts that program. The first word of a command is compared, byte for byte, with the names under `[project.scripts]` and `[tool.poetry.scripts]` whose function is a program entry point. The CI step is then a consumer of `package::pypi::<package>`, with the command as written after `run` (`run core-publish --to prod`), and its link goes on to the function the script names. The level follows the evidence of which package runs: `exact` when an earlier command of the same CI job, on an earlier line or in an earlier CI step, installs the package by a literal name; `exact` when the command names the package (`uvx <package>`, `pipx run <package>`), which runs the script of the package's own name; `inferred` when nothing installs it and one package of the workspace declares the name; `ambiguous`, with a link to each, when several declare it (only to the ones the CI job installed, when it installed some); no side when none declares it. The link is never stronger than the package link it continues, so a package of the CI step's own repository gives `inferred`, and its reason names the script and the evidence after the package's: `package::pypi::core-sdk none (local 1.0.0), range unchecked; console script core-publish, its package installed earlier in the CI job`. `uvx <package>` or `pipx run <package>` for a package with no script of its own name makes the CI step a consumer of the package and starts no program. `trace` crosses the link both ways: up from the program, or from a route it calls, to the CI step, its CI job, its pipeline and the CI jobs that call that pipeline; down from a CI job into the program and what it calls. `context` on the CI step shows the program as the provider, and `context` on the package lists the CI step among its dependents. Limits: the link names the script's function, never the handler of a subcommand; an install in a later CI step, in another CI job, in a called script, by `-e .` or by `-r` is no evidence; only PyPI console scripts are read; a path to the script, `python -m <module>`, `uv run`, `poetry run`, a prefix (`sudo`, `VAR=x`), `uvx --from <package> <script>` and `<package>@<version>` are not matched; a name that more than eight packages of the workspace declare gives no side; and a script of the workspace wins over a shell builtin or a public tool of the same name, at `inferred` when nothing installs its package.

`trace` follows a chain of CI calls both ways, across repositories and through intermediate reusable pipelines and composite actions (a CI step calls an action whose CI step calls another). Up from a reusable pipeline it reaches each CI job that calls it, then the pipeline of that CI job's file and, when that file is reusable too, the CI jobs that call it. Down from a CI job it reaches the pipeline it calls, then that pipeline's CI jobs and CI steps and what they call. The CI contract identifier, the pipeline's file node (`<repository>::<path>::.`) and the file's path start the same chain. A pipeline that reaches itself ends at the walk's `(cycle)` mark, and the walk enters only repositories of the workspace.

The console and server mode show the kind as the CLI and MCP over stdio do. The console's graph has the view **CI links**: the CI contracts with their sides and the link of each call, with the Git ref in its reason and the absent mark, and the playground takes the kind `ci`. `context` on a pipeline, an action, a CI job or a CI step lists its sides above its values, so a long pipeline shows who calls it under the default budget. In server mode (`meada mcp --listen`), MCP over HTTP answers every query of the kind as stdio does over the same checkouts, but for the indexed commit: it is the one `meada index --workspace` recorded, and that command records none for a checkout that is not clean (`git status` lists a file, even an ignored one), so a link to that provider reads `@<ref> (indexed commit unknown)` and never carries `ref mismatch`, where stdio names the checkout's `HEAD`. The sides follow the manifest, the grant, the generation and the named workspace: one that does not hold the provider's repository shows no link to it and no consumer of it, in `context`, `trace`, the graph and the export.

## Communities and processes

A community groups symbols that call each other more than the rest. `meada export` computes these groups with Leiden, without randomness and using integer arithmetic. Names follow the directory containing most members (`src`, `src#2`), identically on every machine.

Each link has level `exact`, `declared`, `inferred`, `decided` or `ambiguous`. An unresolved URL becomes a link only after a decision, with level `decided`. A route whose serving location is unknown stays out of the map.

A process is the call tree from an entry point, across repositories. Routes, channel handlers and exported functions or methods without callers in their own repository are entry points. These also qualify:

- Python: `if __name__ == "__main__":` (symbol `__main__`) and `[project.scripts]` functions in `module:function` form. Exported names are public, without a leading `_`, or listed in a literal `__all__`.
- Rust: each binary's `fn main` in `src/main.rs`, `src/bin/` or `[[bin]] path`.
- Go: `func main` in a `package main`.
- Swift: a type marked `@main` (through its `main()` method when it declares one) and the top-level code of a `main.swift` file, in a symbol `main` that owns the calls written there.

`trace` with `process::<entry point identity>` reads the tree. `meada export` includes each process. `search` with kind `community` finds communities by name; kind `process` finds processes by their entry-point label; kind `duplicate` lists the symbols whose text is identical in two or more places of the workspace (across repositories or within one), as groups ordered by span length, matched by a member's name or path. The export carries each symbol's BLAKE3 `hash`; near copies (a whitespace difference) are not duplicates. `context` reports the symbol's community (or `community: none`) and processes, walking calls and links toward callers.

## Limits and next steps

Languages and HTTP or messaging clients outside these lists are not yet supported. [ADR 0003](https://github.com/ClaudioMascaro/meada/blob/main/docs/adr/0003-v1-scope-and-roadmap.md) defines scope and order. See the [roadmap](roadmap.md), [decisions](https://github.com/ClaudioMascaro/meada/blob/main/docs/adr) and [CHANGELOG](https://github.com/ClaudioMascaro/meada/blob/main/CHANGELOG.md).

## AWS SDK calls in code

Resource reads retain each SDK call as a distinct side in JS, TS and Python. Tracing up reaches its writers; tracing down from a writer reaches the reader function and continues through its calls, with the actual location of each operation.

TypeScript and JavaScript recognize v3 commands sent with `client.send(new Command(...))`,
with both client and command imported from the corresponding AWS package. Python recognizes
methods on clients constructed by `boto3.client(...)` or a `Session`. Names can come from
constants and arguments of exactly resolved wrappers. A client reached only by a name match,
or hidden by a parameter, `catch`, destructuring or another binding, contributes no side.

| Service | Produces or writes | Reads | Name argument |
| --- | --- | --- | --- |
| SNS | `Publish`, `PublishBatch` | | `TopicArn` |
| EventBridge | `PutEvents` | | `Entries[].EventBusName`, `default` when omitted |
| Kinesis | `PutRecord`, `PutRecords` | | `StreamARN` or `StreamName` |
| S3 | `PutObject`, `DeleteObject` | `GetObject`, `HeadObject`, `ListObjects`, `ListObjectsV2` | `Bucket` |
| DynamoDB | `PutItem`, `UpdateItem`, `DeleteItem`, `BatchWriteItem`, `TransactWriteItems` | `GetItem`, `Query`, `Scan`, `BatchGetItem`, `TransactGetItems` | `TableName`, `RequestItems` keys, or `TransactItems` tables |
| SSM | `PutParameter`, `DeleteParameter`, `DeleteParameters` | `GetParameter`, `GetParameters`, `GetParametersByPath` | `Name`, `Names`, `Path` |
| Secrets Manager | `PutSecretValue`, `UpdateSecret`, `CreateSecret`, `DeleteSecret` | `GetSecretValue`, `BatchGetSecretValue` | `SecretId`, `SecretIdList`, `Name` (`CreateSecret`) |

In JavaScript each operation above has the `Command` suffix; boto3 uses snake case
(`put_object`, `get_secret_value`). SNS, EventBridge and Kinesis are producers; S3, DynamoDB,
SSM and Secrets Manager use resource writer and reader sides. Literal names and supported ARNs join infrastructure by
physical name, without account or region. Literal SDK sides link at `exact`, including other
SDK sides in the same repository; a side declared in `meada.toml` keeps `declared`.
Overridable infrastructure values still weaken a link to `inferred`.

Without a matching workload environment, a read such as `process.env.BUCKET` or `os.getenv("BUCKET")` stays unresolved, with
`via: "env:BUCKET"` in the export and the variable named in its function's context. An SDK
producer or writer also has `via`, optional in format 1; a literal uses `sdk:aws`.
Meada never reads the process environment. Constructing an unused command, SNS publishes to
phone numbers or endpoints, high-level `boto3.resource` and `DynamoDBDocumentClient` clients,
and operations outside this table are not recognized by these forms.

`GetParametersByPath` queries a hierarchy, so its side stays unresolved; the path is not treated as a parameter name. A `ConditionCheck` in `TransactWriteItems` is a reader, even when the transaction also writes other tables. Computed `RequestItems` keys retain a literal or the environment variable name.

## Images and workload code

The `image::<registry>/<repository>` contract connects a workload to the repository whose
configuration builds its image (#662). `nginx:1.27` becomes `image::docker.io/library/nginx`;
`acme/api` becomes `image::docker.io/acme/api`. Names are lowercase, without tags or digests.

Builders come from `docker/build-push-action` (`tags`), `docker/metadata-action` (`images`),
Compose with `build` and `image`, Skaffold artifacts (including multiple YAML documents),
literal Maven/Gradle Jib destinations and ko `build --bare` or `publish --bare` commands with
`KO_DOCKER_REPO` set in the workflow. Base images are not destinations. Other ko naming modes
and unread Gradle expressions do not create a guessed builder.

In a pipeline the builder's site is the CI step that holds the form (#833): the workload's link
names that CI step, `trace` from the workload shows it, and `trace` with `direction=up` from the
CI step reaches the workloads that run what it builds. Several CI steps of one repository that
build the same image are one builder, with a link each. A `run` script joins the forms through
the command table: `docker push <image>` builds that image, `docker buildx build` with `--push`
builds each image its `-t`, `--tag` and `--tag=` options name, and a ko command is read wherever
the table reads a command. A tag or digest left open is dropped with the tag, a part of the name
no literal `env` gives makes a pattern (`ghcr.io/${{ github.repository_owner }}/api` is
`ghcr.io/*/api`), and a name left open whole is an unresolved builder that links to nothing.
`docker build`, a `docker buildx build` without `--push`, `docker image push`, `--output
type=registry`, `docker buildx bake`, `docker compose push` and other tools (Podman, Buildah,
Kaniko, crane) are not builders, and a CI job is no workload: its `container`, its `services`
and its `env` give no runner.

Runners are Kubernetes workload containers and init containers, after Kustomize image
overrides, Compose services and Helm values files' `image.repository`. Helm templates are not
rendered. `context` shows the image and its builders; `trace` crosses the relationship
both ways, reaching code entry points. Overlays using the same workload and image retain a
runner and its link per evaluation root. Repeated evidence of that image in one workload and
root forms one side. A local Compose build directly runs the code in the same
repository that its Dockerfile takes from the build context (`COPY`, `ADD` and
`RUN --mount=type=bind` sources, in every stage) at `exact`, even without `image`, inside the root
and through no symbolic link, or at `inferred` when the context or the Dockerfile path went through
an overridable value. A Dockerfile that cannot be read leaves the whole context at `inferred`, and
the reason says so. `.dockerignore` is not read.

Literals link at `exact`; Compose defaults and Helm values at `inferred`, with their source in the
reason. Unresolved parts remain patterns and match only literal names at segment boundaries; several
candidates remain `ambiguous` and can receive a decision. `status` lists image orphans only for a
registry and namespace (`ghcr.io/acme`) that holds builders and runners in the visible scope.
Otherwise `search` and `context` mark the side with no counterpart. Export stays at format 1, adding
builder `at` locations and direct-path `executions`; the console graph shows image contracts, their
links and workload code.

### Partially resolved names

A channel, resource or image with unknown parts retains a pattern such as `*-orders`.
`${terraform.workspace}-orders` matches `prod-orders` at `inferred`: at least three literal
letters or digits are required (separators do not count), and each `*` must occupy whole segments
bounded by `-`, `_`, `.` or `/`. A pattern never matches another pattern. Several matching literal
names stay `ambiguous`, all listed, and become open items. `meada decide` records the choice in
`meada.decisions.toml`; the chosen link becomes `decided`. Provisioning decisions belong to the side
containing the pattern and retain its evaluation root.

Configuration-key items use file, line, key and contract metadata, including when the value is redacted or cut.

An image builder pattern with several literal runner names also keeps its own open item in the repository that builds the image.

## Locations of declared resources

A `[[resource]]` `via` accepts symbols (`src/orders.ts::cancelOrder`), IaC addresses and
`<file>#<key path>`. The side belongs to the block or key, reachable through `context` and
`trace`, and its links stay `declared`. Addresses include Terraform/OpenTofu
(`aws_sqs_queue.orders`), Kubernetes/Crossplane (`Deployment.apps/shop/api`), Compose
(`services.api`), CloudFormation/SAM (`template.yaml::Orders`) and Serverless
(`serverless.yml::functions.api`). Use `<file>::<address>` to distinguish files. The location identifies the source block;
the ` (root ...)` qualifier of IaC queries is not a `via` form.
An address matching several source blocks needs the file qualifier; without it the `via`
is stale.

Key paths use dots, indexes (`groups[0].name`) and JSON-quoted keys
(`metadata.labels["app.kubernetes.io/name"]`). In lists, `[key=value]` selects items by exact,
complete value equality, for example
`values.yaml#groups[name=availability].rules[alert=ApiDown].labels.support`.
Locations come from indexed values and their parent key paths.
Null values and empty containers have no indexed value; their `via` is stale.
A selector can match several items; each matching key gets a side. Redacted or cut values
cannot select an item. HCL expressions and templates cannot satisfy selector equality. Files follow the index's rules, including `.meadaignore`; a `via`
never opens a file on its own.

An unmatched `via` is stale and appears in the existing `meada status` lines. With no valid
`via`, the side falls back to the whole repository. This connects labels in Helm values to
routing rules through declarations without rendering templates.

```toml
[[resource]]
kind = "routing"
id = "team-b"
role = "write"
via = ["values.yaml#groups[name=availability].rules[alert=ApiDown].labels.support"]
```

## AWS functions and tasks

Terraform/OpenTofu Lambda runs `image_uri`; SAM runs `ImageUri`, and ECS tasks run each
container image in `container_definitions`, including `jsonencode` expressions. These runners
participate in image contracts, context, traces and orphans. Environment entries stay with
their container and evaluation root, without reaching code belonging to another image.

A local Lambda `filename` or SAM `CodeUri` directly connects code in the same repository.
An exact `data.archive_file.<name>.output_path` reference may follow only its explicit local
`source_dir` or `source_file` input. ZIP files remain opaque locations; their contents are
never extracted. A Serverless handler such as `worker/send.dispatch` selects one existing
local source file (`.ts`, `.js`, `.mjs`, `.cjs` or `.py`); ambiguous extensions remain unread.
New paths must be relative, contain no `..` component and cross no symbolic link. S3, URLs
and remote code do not become local paths.

SAM merges `Globals.Function.Environment.Variables` with the function environment; Serverless
merges `provider.environment` with `functions.<name>.environment`. The function wins, even
when its override is unreadable. Terraform Lambda and ECS also supply their environments.
Known resource references may carry the contract identity without inventing a deployment ARN
or URL. Secrets, dynamic references and external values retain no contents. The code read
resolves at `inferred`, with workload and variable in its reason; `context` and `trace` show
both image execution and direct code paths.

References such as `aws_sqs_queue.orders.url` and `aws_s3_bucket.assets.bucket` identify
the contract without supplying a deployment URL; content attributes such as `value` stay unread.

## Workload environments

An environment read naming a queue, topic or resource, such as `process.env.QUEUE_URL`,
resolves only against entries of the workload running that code. Execution evidence comes
from the image the repository publishes or a checked local workload code path. Containers keep
their own environments. Resolution is `inferred`, with the workload and variable in its reason.

Kubernetes supplies literals, ConfigMap or Secret key references and `envFrom`; Compose supplies
map or list entries and defaults such as `${QUEUE_URL:-orders}`. Workload `context` shows
names, targets and available values. A Secret stays `redacted`, and a missing reference or
unresolved value creates no link. Names match only within the workload's scope: values come
from the repositories' IaC and configuration, never from a process environment.

## Alerts and Alertmanager routes

The `alert::<key>::<value>` contract joins labels on Prometheus and Loki rules
(including `PrometheusRule`) and Grafana rules to Alertmanager route matchers, in
configuration files and `AlertmanagerConfig` objects. Equality creates an `exact` link;
a regular expression creates an `inferred` link. A route's conditions are checked together.
A negated condition (`!=`, `!~`) only filters the route: it creates no side or link for the
value it excludes.

A regular expression that Go's `regexp` cannot compile (an unclosed group `(`, a leading `*`,
a backward range `[z-a]`, a lookahead `(?=`) makes Alertmanager refuse the whole
configuration, so Meada reads that configuration, or that `AlertmanagerConfig` object, as
unreadable: it creates no alert side or link and `meada status` counts the file under
`unreadable`. The same configuration with the expression fixed links as usual. An expression
that Meada leaves unmatched only for its own limits, or because it does not read the syntax,
is valid as far as Meada knows: its matcher shows as an orphan and the other routes keep
linking.

`search` with kind `alert` finds the contract; `context` on a route shows its receiver,
and `trace` walks from a rule to its route and back. `meada export` includes emitters,
matchers and links. `meada status` lists orphans only when the visible scope has both
sides; otherwise `search` and `context` mark the missing side. Helm values awaiting
template rendering are not emitters: Meada does not render templates.

Grafana rules also come from JSON exports and `rule` blocks of Terraform/OpenTofu `grafana_rule_group` resources with literal labels.

A root route without matchers creates no alert side: there is no key and value for a contract. The graph describes written matchers, without predicting final notifications, inhibition or schedules.

Prometheus and Loki also emit the literal `alertname` from the rule name, overriding a label of that name. Labels that may come from the expression remain unknown, not empty. Files under `templates/` are not rendered or treated as alert emitters.

### OpsGenie and PagerDuty in Terraform

Literal `key:value` tags on `datadog_monitor` are emitters. Tag conditions in
`opsgenie_team_routing_rule` and PagerDuty
orchestration PCL conditions, are matchers. Equality is `exact`; regex, prefix and
containment are `inferred`. Conditions and rule order are evaluated together; unknown
values or unread operations do not choose a destination by guess.

Datadog tags reach OpsGenie. Rule labels reach tags only through a selected Alertmanager
OpsGenie receiver that sets `tags` explicitly. Simple `.CommonLabels.<key>` or
`.GroupLabels.<key>` templates use labels fixed by the effective `group_by`. A PagerDuty
v2 receiver can forward these labels through `details`, which becomes
`event.custom_details.<key>`; there is no implicit tag forwarding to PagerDuty. Unread
templates and missing forwardings create no links.

`context` shows the rule's written scope, and `trace` retains the path to its routing
block. A compatible matcher does not establish delivery or a final team notification.
In public example #638 the rule tests `service:app`: the same monitor also emits
`support:team-b`, but there is no `support` matcher there. The original nonstandard
monitor YAML and Helm values remain configuration only.

## Azure contracts and Functions

The Terraform/OpenTofu table reads `azurerm_servicebus_queue`, `azurerm_servicebus_topic`
and `azurerm_servicebus_subscription` as `servicebus` channels; `azurerm_eventhub` as
`eventhubs`; `azurerm_storage_account` as resource `azurestorage`,
`azurerm_storage_container` as `azureblob` and `azurerm_key_vault_secret` as `keyvault`.
An account and container with the same name remain distinct contracts.

A subscription has identity `channel::servicebus::subscription:<topic>/<subscription>`.
It joins its consumers to the topic's producers, at `exact` without a filter rule and
`inferred` when a rule is declared. Queue/topic names retain `/`. Existing C# processors
retain topic identity, including overloads taking a subscription. Complete supported ARM
IDs and blob/secret URLs give `exact` uses; final keys `servicebus_queue`, `servicebus_topic`,
`servicebus_subscription` (value `topic/subscription`), `eventhubs_hub`,
`azurestorage_account`, `azureblob_container` and `keyvault_secret` give `inferred`.
Equal values without that evidence create no link.

`azurerm_linux_function_app` reads the Docker block under `site_config.application_stack`;
both Linux and Windows types read local `zip_deploy_file` and `app_settings`. A local file
stays inside the repository with no `..` or symlink; ZIP archives are never opened. A remote
package identifies no local code. Key Vault app-setting references stay redacted and
unresolved. Contracts and workloads participate in `context`, `trace`, orphans,
provisioning and export.

### Azure SDKs in five language groups

Known Azure SDK constructors in JavaScript/TypeScript, Python, Java, Go and C# create Service Bus and Event Hubs producers/consumers, Blob container writers/readers and Key Vault secret readers. The [user guide](https://github.com/ClaudioMascaro/meada/blob/main/docs/user-guide.en.md#azure-sdk-use-in-five-language-groups) lists the constructors and operations. Subscription receivers retain topic and subscription within the same workload, root and container; environment names without that evidence stay unresolved. Existing C# Service Bus forms retain topic identity.

## Custom resources and infrastructure modules

This is a closed inventory of 43 custom resource types, plus the RabbitMQ operator Exchange and Binding roles. Types and groups must match exactly. Other CRDs remain available through `search` with `kind = "iac"`, `context` and `trace`, without an assumed contract. Meada reads repository files without querying operators or downloading modules.

| Type and group | Identity field | Contract |
| --- | --- | --- |
| `KafkaTopic.kafka.strimzi.io` | `spec.topicName` | `channel::kafka` |
| `Queue.rabbitmq.com` | `spec.name` | `channel::amqp` |
| `Queue.sqs.services.k8s.aws` | `spec.queueName` | `channel::sqs` |
| `Bucket.s3.services.k8s.aws` | `spec.name` | `resource::s3` |
| `Topic.sns.services.k8s.aws` | `spec.name` | `channel::sns` |
| `EventBus.eventbridge.services.k8s.aws` | `spec.name` | `channel::eventbridge` |
| `Stream.kinesis.services.k8s.aws` | `spec.name` | `channel::kinesis` |
| `Table.dynamodb.services.k8s.aws` | `spec.tableName` | `resource::dynamodb` |
| `DBInstance.rds.services.k8s.aws` | `spec.dbInstanceIdentifier` | `resource::rds` |
| `DBCluster.rds.services.k8s.aws` | `spec.dbClusterIdentifier` | `resource::rds` |
| `CacheCluster.elasticache.services.k8s.aws` | `spec.cacheClusterID` | `resource::elasticache` |
| `ReplicationGroup.elasticache.services.k8s.aws` | `spec.replicationGroupID` | `resource::elasticache` |
| `Parameter.ssm.services.k8s.aws` | `spec.name` | `resource::ssm` |
| `Secret.secretsmanager.services.k8s.aws` | `spec.name` | `resource::secretsmanager` |
| `PubSubTopic.pubsub.cnrm.cloud.google.com` | `spec.resourceID` | `channel::pubsub` |
| `PubSubSubscription.pubsub.cnrm.cloud.google.com` | `spec.resourceID` | `channel::pubsub` |
| `StorageBucket.storage.cnrm.cloud.google.com` | `spec.resourceID` | `resource::gcs` |
| `SQLInstance.sql.cnrm.cloud.google.com` | `spec.resourceID` | `resource::cloudsql` |
| `SQLDatabase.sql.cnrm.cloud.google.com` | `spec.resourceID` | `resource::cloudsql` |
| `SecretManagerSecret.secretmanager.cnrm.cloud.google.com` | `spec.resourceID` | `resource::gcpsecretmanager` |
| `Queue.sqs.aws.upbound.io` | `metadata.annotations["crossplane.io/external-name"]` | `channel::sqs` |
| `Bucket.s3.aws.upbound.io` | `metadata.annotations["crossplane.io/external-name"]` | `resource::s3` |
| `Topic.sns.aws.upbound.io` | `metadata.annotations["crossplane.io/external-name"]` | `channel::sns` |
| `Bus.cloudwatchevents.aws.upbound.io` | `metadata.annotations["crossplane.io/external-name"]` | `channel::eventbridge` |
| `Stream.kinesis.aws.upbound.io` | `metadata.annotations["crossplane.io/external-name"]` | `channel::kinesis` |
| `Table.dynamodb.aws.upbound.io` | `metadata.annotations["crossplane.io/external-name"]` | `resource::dynamodb` |
| `Instance.rds.aws.upbound.io` | `metadata.annotations["crossplane.io/external-name"]` | `resource::rds` |
| `Cluster.elasticache.aws.upbound.io` | `metadata.annotations["crossplane.io/external-name"]` | `resource::elasticache` |
| `Parameter.ssm.aws.upbound.io` | `metadata.annotations["crossplane.io/external-name"]` | `resource::ssm` |
| `Secret.secretsmanager.aws.upbound.io` | `metadata.annotations["crossplane.io/external-name"]` | `resource::secretsmanager` |
| `Topic.pubsub.gcp.upbound.io` | `metadata.annotations["crossplane.io/external-name"]` | `channel::pubsub` |
| `Subscription.pubsub.gcp.upbound.io` | `metadata.annotations["crossplane.io/external-name"]` | `channel::pubsub` |
| `Bucket.storage.gcp.upbound.io` | `metadata.annotations["crossplane.io/external-name"]` | `resource::gcs` |
| `DatabaseInstance.sql.gcp.upbound.io` | `metadata.annotations["crossplane.io/external-name"]` | `resource::cloudsql` |
| `Database.sql.gcp.upbound.io` | `metadata.annotations["crossplane.io/external-name"]` | `resource::cloudsql` |
| `Secret.secretmanager.gcp.upbound.io` | `metadata.annotations["crossplane.io/external-name"]` | `resource::gcpsecretmanager` |
| `Queue.servicebus.azure.upbound.io` | `metadata.annotations["crossplane.io/external-name"]` | `channel::servicebus` |
| `Topic.servicebus.azure.upbound.io` | `metadata.annotations["crossplane.io/external-name"]` | `channel::servicebus` |
| `Subscription.servicebus.azure.upbound.io` | `metadata.annotations["crossplane.io/external-name"]` | `channel::servicebus` |
| `EventHub.eventhub.azure.upbound.io` | `metadata.annotations["crossplane.io/external-name"]` | `channel::eventhubs` |
| `Account.storage.azure.upbound.io` | `metadata.annotations["crossplane.io/external-name"]` | `resource::azurestorage` |
| `Container.storage.azure.upbound.io` | `metadata.annotations["crossplane.io/external-name"]` | `resource::azureblob` |
| `Secret.keyvault.azure.upbound.io` | `metadata.annotations["crossplane.io/external-name"]` | `resource::keyvault` |

Strimzi uses `metadata.name` only when `spec.topicName` is absent. Config Connector uses `metadata.name` only when `spec.resourceID` is absent. A present but unknown or invalid field does not enable that default. The listed Crossplane families require an explicit `crossplane.io/external-name` annotation. The physical name shown on a node stays separate from its normalized contract identifier.

Cloud SQL uses the composite identity `<instance>/<database>`: `SQLDatabase` reads `spec.instanceRef`, and `Database.sql.gcp.upbound.io` reads `spec.forProvider.instance` or `instanceRef`. Local references must resolve to one object of the correct group, type and namespace. Pub/Sub subscriptions have a separate `channel::pubsub::subscriptions/<name>` contract and read their source from `spec.topicRef` or `spec.forProvider.topic`/`topicRef`. Service Bus subscriptions require a full name or ARM ID identifying both topic and subscription, producing `channel::servicebus::subscription:<topic>/<subscription>`. An unqualified short name is insufficient.

Literal Composition bases use the same inventory. Patches that can change identity, qualification or routing suppress projection; unknown patches, absent or duplicate patch sets and nested sets also establish no contract. Functions and patches are never executed.

### RabbitMQ topology

`Queue.rabbitmq.com` provisions `channel::amqp::<spec.name>`. `Exchange.rabbitmq.com` supplies its name and type without becoming a channel. `Binding.rabbitmq.com` references `spec.source` and `spec.destination` in the same evaluation unit, cluster, namespace and vhost; a `connectionSecret` reference stays distinct from a cluster-name reference. Only `queue` destinations with unique, known endpoints produce a binding. `topic` uses `*` and `#`; `direct` accepts keys without wildcards; `fanout` reaches all keys. `headers`, unknown types and `exchange` destinations produce no queue route. The latter still have navigable references. An absent Exchange type defaults to `direct`. AMQP names and keys are limited to 255 bytes. Provisioning from a publish retains its real site and route level, `exact` for a literal key and `inferred` for a wildcard.

### Registry modules

Only these six sources are recognized; the optional `registry.terraform.io/` prefix is accepted. The table lists the inputs read and creation controls.

| Source | Inputs; controls |
| --- | --- |
| `terraform-aws-modules/sqs/aws` | `name`, `fifo_queue`, `use_name_prefix`; `create` |
| `terraform-aws-modules/sns/aws` | `name`, `fifo_topic`, `use_name_prefix`; `create` |
| `terraform-aws-modules/s3-bucket/aws` | `bucket`; `create`, `create_bucket`, `putin_khuylo` |
| `terraform-aws-modules/dynamodb-table/aws` | `name`; `create`, `create_table` |
| `terraform-aws-modules/lambda/aws` | `function_name`, `package_type`, `image_uri`, `environment_variables`, `source_path`, `local_existing_package`, `handler`; `create`, `create_function`, `create_package`, `create_layer`, `putin_khuylo` |
| `terraform-aws-modules/ecs/aws//modules/service` | `name`, `container_definitions` (`name`, `create`, `image`, `environment`, `secrets`); `create`, `create_service`, `create_task_definition` |

`create = false`, `count = 0` or an unknown creation control produces no side. Unknown names remain safe patterns without an invented physical name. SQS and SNS append `.fifo` when requested; names generated by `use_name_prefix` establish no literal provisioner. Lambda and ECS supply images and environments through the same reader as native workloads, retaining container scopes. Lambda accepts a known scalar `source_path` with `create_package = true`, inside the repository and through no `..` or symlink; list/object forms and S3 packaging have no local target. `local_existing_package` must be an existing file and stays opaque: no ZIP extraction or packager execution. `create_layer = true` suppresses the function. The module directory is never presumed to be the function code.
