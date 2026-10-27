# Dogfood repositories

Public community repositories, one or two per language pack, cloned shallowly into
`~/codes/dogfood/` and used to dogfood, validate and benchmark every pack on real code. The
maintainer's private repositories stay the private benchmark of ADR 0003; this set exists
because none of them is written in Java, Kotlin, C# or Swift.

## Safety rule

The clones are untrusted data. Index only, never build or run: no `npm`, `pip`, `cargo`,
`gradle`, `maven`, `dotnet` or `swift` command inside them, no interpreter started from inside
them, no editor hooks. Only `meada index`, `meada export` and read-only shell commands touch
them.

## Set

Measured on 2026-10-27 with `meada` built from this tree (Swift pack included), cold index.
Lines count the pack's language only. Definitions are `symbols` of `meada export`; calls are
the export's `calls` by resolution level (`unresolved` is a null level).

| Pack | Repository | Commit | Lines | Index (s) | Defs | exact | inferred | ambiguous | unresolved | Exercises |
|---|---|---|---|---|---|---|---|---|---|---|
| python | fastapi/full-stack-fastapi-template | f4bcfaba | 2,901 | 0.27 | 467 | 220 | 0 | 0 | 2,510 | FastAPI routes |
| ts | expressjs/express | 1c6f1b0f | 22,046 (js) | 0.74 | 125 | 309 | 6 | 0 | 11,757 | JavaScript routes |
| ts | honojs/hono | bac5c4a8 | 75,967 | 1.88 | 1,589 | 5,828 | 3,825 | 638 | 33,008 | large TypeScript |
| go | gin-gonic/gin | 0f09c3a9 | 24,556 | 0.45 | 1,540 | 3,130 | 67 | 866 | 5,374 | Go routing |
| rust | tokio-rs/axum | ff653461 | 47,647 | 1.21 | 1,931 | 1,347 | 323 | 1,205 | 5,992 | Rust workspace |
| java | spring-projects/spring-petclinic | 500158f7 | 4,297 | 0.22 | 245 | 77 | 279 | 25 | 1,537 | Spring Data bases, #353 |
| kotlin | spring-petclinic/spring-petclinic-kotlin | 817dba3b | 2,180 | 0.17 | 141 | 30 | 81 | 0 | 641 | twin of the Java one |
| csharp | jasontaylordev/CleanArchitecture | 345e7fe7 | 3,734 | 0.35 | 470 | 175 | 86 | 44 | 1,214 | `ControllerBase`, `IDisposable`, #353 |
| csharp | dotnet/eShop | 913acea6 | 26,350 | 1.89 | 1,885 | 960 | 366 | 259 | 6,640 | `ControllerBase`, `IDisposable`, #353 |
| swift | Alamofire/Alamofire | bda9ed57 | 43,746 | 1.39 | 2,623 | 2,077 | 1,733 | 2,526 | 12,344 | `NSObject`, `URLSession`, #353, #385 |
| swift | hummingbird-project/hummingbird-examples | 8fb9bc3d | 15,793 | 0.66 | 1,206 | 514 | 143 | 211 | 4,086 | route groups and collections, #386, #387 |

Swap: `nestjs/nest` has 215k lines of TypeScript, over the 150k limit, so `honojs/hono` took its
place.

## Rebuild

```sh
mkdir -p ~/codes/dogfood && cd ~/codes/dogfood
for r in fastapi/full-stack-fastapi-template expressjs/express honojs/hono gin-gonic/gin \
  tokio-rs/axum spring-projects/spring-petclinic spring-petclinic/spring-petclinic-kotlin \
  jasontaylordev/CleanArchitecture dotnet/eShop Alamofire/Alamofire \
  hummingbird-project/hummingbird-examples; do
  git clone --depth 1 "https://github.com/$r.git" "${r//\//-}"
done
```

Clones track the default branch; check out the commits above to reproduce the numbers.
Then run `meada index` inside each directory.
