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

| Pack | Repository | Commit | Lines | Index (s) | RSS | Defs | exact | inferred | ambiguous | unresolved | Exercises |
|---|---|---|---|---|---|---|---|---|---|---|---|
| python | fastapi/full-stack-fastapi-template | f4bcfaba | 2,901 | 0.27 | n/a | 467 | 220 | 0 | 0 | 2,510 | FastAPI routes |
| python | django/django | e9cf067c | 530,078 | 14.11 | 536 MB | 44,374 | 72,360 | 11,525 | 36,274 | 83,016 | large Python, class bases |
| python | fastapi/fastapi | 79cab5be | 131,859 | 2.31 | 111 MB | 7,132 | 7,200 | 125 | 392 | 15,623 | FastAPI library |
| python | encode/django-rest-framework | 14998d0a | 42,405 | 0.99 | 59 MB | 4,830 | 4,651 | 449 | 1,365 | 8,131 | DRF views and serializers |
| ts | expressjs/express | 1c6f1b0f | 22,046 (js) | 0.74 | n/a | 125 | 309 | 6 | 0 | 11,757 | JavaScript routes |
| ts | honojs/hono | bac5c4a8 | 75,967 | 1.88 | n/a | 1,589 | 5,828 | 3,825 | 638 | 33,008 | large TypeScript |
| ts | nestjs/nest | 6ce26bc6 | 215,608 | 8.39 | 180 MB | 8,199 | 14,847 | 6,429 | 15,141 | 52,685 | decorators, DI (back from the swap) |
| ts | typeorm/typeorm | 5bdc6788 | 305,047 | 12.76 | 206 MB | 6,144 | 22,700 | 6,626 | 18,069 | 36,324 | mid-size TypeScript |
| ts | vercel/next.js | 433fe0ed | 729,537 | 30.05 | 920 MB | 49,315 | 60,586 | 50,323 | 65,699 | 206,931 | large TypeScript benchmark |
| go | gin-gonic/gin | 0f09c3a9 | 24,556 | 0.45 | n/a | 1,540 | 3,130 | 67 | 866 | 5,374 | Go routing |
| go | gofiber/fiber | a362028a | 194,917 | 4.12 | 222 MB | 7,773 | 27,834 | 1,417 | 7,213 | 52,606 | Go routing |
| go | gohugoio/hugo | de8237b6 | 234,044 | 3.31 | 170 MB | 10,689 | 18,580 | 1,630 | 5,270 | 33,548 | mid-size Go |
| rust | tokio-rs/axum | ff653461 | 47,647 | 1.21 | n/a | 1,931 | 1,347 | 323 | 1,205 | 5,992 | Rust workspace |
| rust | actix/actix-web | 88d7742d | 89,029 | 4.12 | 65 MB | 4,003 | 3,690 | 1,733 | 3,836 | 9,768 | Rust web framework |
| rust | tokio-rs/tokio | 0d38f834 | 189,596 | 1.38 | 55 MB | 763 | 190 | 92 | 179 | 1,139 | async runtime, macros |
| rust | rust-lang/cargo | 31d3246e | 349,148 | 4.82 | 176 MB | 6,195 | 8,529 | 4,405 | 7,034 | 22,953 | mid-size Rust |
| rust | bevyengine/bevy | db44e0b0 | 692,066 | 21.91 | 342 MB | 28,836 | 19,060 | 10,201 | 37,044 | 43,008 | large Rust benchmark |
| java | spring-projects/spring-petclinic | 500158f7 | 4,297 | 0.22 | n/a | 245 | 77 | 279 | 25 | 1,537 | Spring Data bases, #353 |
| java | junit-team/junit5 | ca61d8b0 | 228,772 | 9.73 | 317 MB | 19,670 | 35,309 | 4,786 | 18,273 | 37,720 | Java interfaces, annotations |
| java | apache/dubbo | 843b98b3 | 481,594 | 17.15 | 486 MB | 35,173 | 43,879 | 17,688 | 21,251 | 83,091 | mid-size Java |
| java | spring-projects/spring-boot | 6922eaaa | 779,837 | 49.91 | 1235 MB | 71,265 | 72,158 | 18,642 | 54,562 | 197,669 | large Java benchmark |
| kotlin | spring-petclinic/spring-petclinic-kotlin | 817dba3b | 2,180 | 0.17 | n/a | 141 | 30 | 81 | 0 | 641 | twin of the Java one |
| kotlin | square/okhttp | a3156ec8 | 140,950 | 7.69 | 494 MB | 8,032 | 8,965 | 10,262 | 10,300 | 30,874 | Kotlin library |
| kotlin | android/nowinandroid | 2982e63c | 29,336 | 1.23 | 80 MB | 1,294 | 863 | 765 | 159 | 5,323 | Android Kotlin app |
| kotlin | Kotlin/kotlinx.coroutines | 7e0d1b3e | 113,379 | 8.41 | 555 MB | 8,473 | 1,840 | 8,048 | 19,689 | 17,022 | Kotlin multiplatform |
| kotlin | ktorio/ktor | d9628016 | 298,944 | 25.46 | 1174 MB | 19,041 | 9,620 | 17,098 | 39,085 | 39,720 | large Kotlin benchmark |
| csharp | jasontaylordev/CleanArchitecture | 345e7fe7 | 3,734 | 0.35 | n/a | 470 | 175 | 86 | 44 | 1,214 | `ControllerBase`, `IDisposable`, #353 |
| csharp | dotnet/eShop | 913acea6 | 26,350 | 1.89 | n/a | 1,885 | 960 | 366 | 259 | 6,640 | `ControllerBase`, `IDisposable`, #353 |
| csharp | jellyfin/jellyfin | 02c2951e | 365,970 | 18.37 | 798 MB | 11,566 | 11,479 | 6,606 | 3,670 | 91,525 | mid-size C#, `ControllerBase`, #353 |
| swift | Alamofire/Alamofire | bda9ed57 | 43,746 | 1.39 | n/a | 2,623 | 2,077 | 1,733 | 2,526 | 12,344 | `NSObject`, `URLSession`, #353, #385 |
| swift | hummingbird-project/hummingbird-examples | 8fb9bc3d | 15,793 | 0.66 | n/a | 1,206 | 514 | 143 | 211 | 4,086 | route groups and collections, #386, #387 |
| swift | vapor/vapor | 0f2a8ee9 | 42,249 | 1.47 | 84 MB | 2,418 | 2,816 | 1,380 | 1,766 | 5,146 | route groups and collections, #386, #387 |
| swift | hummingbird-project/hummingbird | 4f68f133 | 29,440 | 1.15 | 67 MB | 2,083 | 1,399 | 1,250 | 867 | 4,578 | route groups and collections, #386, #387 |
| swift | apple/swift-nio | 108daca3 | 209,202 | 7.47 | 237 MB | 11,207 | 7,861 | 14,318 | 12,149 | 30,418 | protocol-heavy Swift, #353 |
| swift | pointfreeco/swift-composable-architecture | bb175b54 | 83,384 | 2.37 | 86 MB | 4,762 | 3,171 | 1,096 | 3,827 | 15,892 | macros, generics |
| swift | swiftlang/swift-package-manager | 7dd21094 | 359,845 | 12.72 | 420 MB | 13,866 | 18,453 | 13,396 | 16,893 | 43,953 | large Swift, `NSObject` |

## Limits

There is no line ceiling. A shallow clone is skipped when GitHub reports more than about 400,000
KB for the repository (`gh api repos/<owner>/<repo> --jq .size`) or when it passes 1 GB on disk.
The machine has 7 GB of RAM and 3 GB of swap, so every `meada index` runs alone, inside its own
scope, never with a build in parallel:

```sh
systemd-run --user --scope -q -p MemoryMax=2G -p MemorySwapMax=0 -- \
  timeout 600 /usr/bin/time -f "%e s, %M KB max RSS" meada index
```

The 2 GB cap is the default. A repository that hits it is recorded below and not retried; only
a single probe of a known-oversized repository may run at 3500M, alone with at least 4000 MB
available, as `kubernetes/kubernetes` did. Rows without an RSS were measured before the cap
existed. Skipped on size: `google/guava` (1.4 GB
on GitHub), `dotnet/aspnetcore`, `mozilla-mobile/firefox-ios`. `nestjs/nest`, swapped out in the
first round for its 215k lines, is back.

## Ceiling findings

Candidates for issues, not opened.

- `kubernetes/kubernetes` (commit 3e6f6b9a, 422 MB): 5,476,634 lines of Go, 4,022,054 without
  `vendor/`. Three runs, none finished. Uncapped: killed by the kernel at 5.2 GB RSS (7.2 GB
  virtual), leaving a 3.2 GB `index.db.new.*`. At 3500M with swap off: OOM-killed at about 3.4 GB
  RSS after 74 s. With `vendor/` removed, same cap: OOM-killed after about 62 s (sampled peak 2.8
  GB, the kill came between samples). Not indexed, not retried.
- `bitwarden/server` (2,134,048 lines of C#, 131 MB, commit 2346d34e): `meada index` passed the
  600 s timeout without finishing (exit 143). Not retried.
- `dotnet/efcore` (1,774,591 lines of C#, 132 MB, commit 203a4ec9): not a Rust panic: `thread
  'main' has overflowed its stack`, abort after 24.5 s at 1.2 GB RSS (exit 134). A stack
  overflow on deeply nested C# is a bug candidate.

## Rebuild

```sh
mkdir -p ~/codes/dogfood && cd ~/codes/dogfood
for r in fastapi/full-stack-fastapi-template expressjs/express honojs/hono gin-gonic/gin \
  tokio-rs/axum spring-projects/spring-petclinic spring-petclinic/spring-petclinic-kotlin \
  jasontaylordev/CleanArchitecture dotnet/eShop Alamofire/Alamofire \
  hummingbird-project/hummingbird-examples \
  django/django fastapi/fastapi encode/django-rest-framework nestjs/nest typeorm/typeorm \
  vercel/next.js gofiber/fiber gohugoio/hugo kubernetes/kubernetes actix/actix-web \
  tokio-rs/tokio rust-lang/cargo bevyengine/bevy junit-team/junit5 apache/dubbo \
  spring-projects/spring-boot square/okhttp android/nowinandroid Kotlin/kotlinx.coroutines \
  ktorio/ktor bitwarden/server jellyfin/jellyfin dotnet/efcore vapor/vapor \
  hummingbird-project/hummingbird apple/swift-nio pointfreeco/swift-composable-architecture \
  swiftlang/swift-package-manager; do
  git clone --depth 1 "https://github.com/$r.git" "${r//\//-}"
done
```

Clones track the default branch; check out the commits above to reproduce the numbers.
Then run `meada index` inside each directory, one at a time, with the scope above.
