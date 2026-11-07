# Contract modules and query declarations

- `contract.rs`, `http.rs` and its `http/` submodules, `resource.rs`, `channel.rs` and its `channel/` submodules
  (`channel/message.rs`: the message types and broker helpers; `channel/{ts,python,rust,go,java,csharp}.rs`:
  the message libraries of each language): the
  `ContractKind` trait and the HTTP kind (`http.rs`: the contract and path rules, the mount
  machinery and the shared consumer readers; one submodule per language: `http/ts.rs` and
  `http/ts/clients.rs`, Express and Next.js route-file providers and `fetch` consumers, with
  `http/ts/backstage.rs` mounting each Backstage backend plugin's router at `/api/<pluginId>` (a
  mount node per `createBackendPlugin` call in Express's `Mounts`) and folding a
  `discovery.getBaseUrl('<id>')` base to `/api/<id>` (a `client:` `via`, so the reason names
  it; `backstage::name` gives the same `via` to a hostless consumer of a route a plugin's mount
  serves, from the routes `ts::providers` reports as mounted; a plugin whose `pluginId` is
  neither a literal nor a `const` of the file bound to one is a `Role::Unmounted` row that
  `Http::status` reports; `http/ts/config.rs` reads the `proxy.endpoints` of `app-config*.yaml`
  (#616): a URL that starts with `/api/proxy/<ep>` is rewritten to the endpoint's `target`, the
  consumer's `via` keeping the endpoint after `PROXY_VIA` for the reason, and an endpoint whose
  target is not read gives the `client:` `via` that `backstage::unread` finds for `status`);
  `http/python.rs` and `http/python/clients.rs`, FastAPI, Flask and Django providers and the
  requests, httpx and aiohttp consumers, whose rows also list reqwest and ureq; `http/rust.rs`
  and `http/rust/axum.rs`, actix-web, Rocket and axum providers; `http/go.rs` and
  `http/go/clients.rs`, net/http, gin, echo, chi and gorilla/mux providers and their consumers
  with resty; `http/java.rs` and `http/java/clients.rs`, Spring MVC providers and the
  RestTemplate, WebClient, `java.net.http` and OkHttp consumers, whose bases come from the
  Spring configuration `jvm/spring.rs` reads for the JVM packs' kinds; `http/kotlin.rs`, Ktor
  providers, and `http/kotlin/client.rs`, Ktor client consumers; `http/csharp.rs`, ASP.NET Core
  controller providers, `http/csharp/minimal.rs`, minimal API providers,
  `http/csharp/clients.rs`, HttpClient and typed client consumers, and `http/csharp/refit.rs`,
  Refit consumers; `http/swift.rs`, `http/swift/vapor.rs`, Vapor providers, `http/swift/hummingbird.rs`,
  Hummingbird providers, and `http/swift/client.rs`, URLSession,
  Alamofire and AsyncHTTPClient consumers), resource and channel
  kinds. `package.rs` and `package/`: npm, PyPI, Cargo, Go, Maven, NuGet and SwiftPM packages as
  contracts (specs #22, #24, #30, #25, #26, #28 and #29), the shared steps and one module per ecosystem behind
  the `Ecosystem` trait (spec #225).


- `http.rs` `declare` first drops the providers whose site is in a test file (`is_test_file`,
  #620) and the route entry points of those sites, unless `meada.toml` has `test_routes = true`
  (`Config::test_routes`, a top-level key a workspace manifest entry does not carry). It runs at
  query time, so the key needs no reindex; the test code stays indexed.
