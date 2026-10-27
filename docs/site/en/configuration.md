# Configuration

Each repository can have a `meada.toml` at its root, versioned in git, saying who it talks to.
A repository that consumes a route names its peer:

```toml
[[peer]]
repo = "example.com/acme/api"            # origin git remote, no scheme and no .git
base_url_env = ["API_URL"]               # variable the client reads its base URL from
# path = "../api"                        # optional: a hint of where the clone is
```

A repository that serves the route declares the hosts it answers on:

```toml
[serve]
hosts = ["api.example.com"]
prefix = "/shop"                         # optional: what the gateway adds in front of paths
strip = "/v1"                            # optional: what it removes from their start
```

With `prefix` and `strip`, a call to `/shop/orders` reaches the code route `/v1/orders`, at the
level `inferred` with the reason `gateway prefix /shop, gateway strip /v1`. A version segment
goes only when you write it in `strip`, and callers of the code path keep linking as before.

A shared resource no code names, such as a table or a bucket, goes in as `[[resource]]`:

```toml
[[resource]]
kind = "postgres"                        # lowercase identifier
id = "orders"
role = "write"                           # read, write or read-write
via = ["src/orders.ts::cancelOrder"]     # optional: the symbols that do the access
```

Every writer links to every reader of the same resource in another repository, at the level
`declared`. Without `via`, or when no `via` matches a symbol, the side is the whole repository.

Meada finds peers by their git remote, even when the clone sits at a different path on each
machine. An unknown or misspelled key gives an error naming the file, the line and the key.
Without a `meada.toml`, the repository declares no peers.

## JVM artifacts

JVM package contracts are discovered from manifests: `pom.xml`, `settings.gradle`, `settings.gradle.kts`, `build.gradle` and `build.gradle.kts`. They need no additional `meada.toml` key. Declare other repositories in `[[peer]]` as in the examples above. Use literal coordinates for `exact` links; Maven properties and Gradle expressions remain unevaluated, and those that make up the group or artifact produce `ambiguous` candidates.

In Java and Kotlin with Spring, the base URL may come from `@Value("${key}")` (in Kotlin, `@Value("\${key}")`, the `$` escaped), read from the `application.properties`, `application.yml` or `application.yaml` in each module's `src/main/resources`, with no new `meada.toml` key. A literal URL links by host in `[serve] hosts`; a `${VAR}` placeholder (with or without `:default`) uses the same `[[peer]]` `base_url_env` keys. A default whose host some peer serves links by that host.

In C#, the base URL may come from `Environment.GetEnvironmentVariable("X")` or from `configuration["Section:Key"]`, read from the project's `appsettings.json` (the base file only), with no new `meada.toml` key. A key is the variable `Section__Key` in `[[peer]]` `base_url_env`; a literal URL the file holds for it links by host in `[serve] hosts` when some peer serves that host.

## NuGet packages

NuGet package contracts are discovered from the SDK-style `.csproj` files. They need no additional `meada.toml` key: a `PackageReference` links to a provider by its id, and a `ProjectReference` by the path of its project file, which reaches a project of another repository by the roots of the workspace repositories (declare it in `[[peer]]`). Property values such as `$(Version)` stay unevaluated.

## Swift packages

Swift package contracts are discovered from `Package.swift`. They need no additional `meada.toml` key: a `.package(url:)` links to the workspace repository with the same remote identity and a `.package(path:)` to the repository at that path, which must be a `[[peer]]` of the workspace when it is another repository. Use literal URLs, paths and names for `exact` links.

