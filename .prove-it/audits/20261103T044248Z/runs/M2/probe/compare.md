# prove.py run

differences found: 1

## meada

meada 1.1.0 (base) vs meada 1.1.0 (head)

Determinism: cold twice equal base True, head True; refresh equal base True, head True
Budget:
  index seconds: 44.835 -> 44.39 (limit x1.25)
  peak RSS KiB: 237128 -> 236872 (limit x1.25)
  export bytes: 28672668 -> 28672668 (limit x1.5)
Export, base -> head:
  identical
MCP context {"symbol": "github.com/claudiomascaro/meada::src/trace.rs::Walk.push", "budget": 1}: differs
```
--- base
+++ head
@@ -1 +1,80 @@
-… cut after 0 of 414 lines (budget 1 tokens); raise budget for more
+github.com/claudiomascaro/meada src/trace.rs:712-716 Walk.push
+community: src/lang
+processes: 3 (2 export, 1 program; 1 repository): ready (github.com/claudiomascaro/meada), status_workspace (github.com/claudiomascaro/meada), main (github.com/claudiomascaro/meada)
+callers:
+github.com/claudiomascaro/meada src/channel.rs:204 Channel.ContractKind.extract [inferred: one method named push]
+github.com/claudiomascaro/meada src/channel/csharp.rs:602 tests.lines_with [inferred: one method named push]
+github.com/claudiomascaro/meada src/channel/java.rs:649 listen [inferred: one method named push]
+github.com/claudiomascaro/meada src/channel/java.rs:661 listen [inferred: one method named push]
+github.com/claudiomascaro/meada src/channel/java.rs:667 listen [inferred: one method named push]
+github.com/claudiomascaro/meada src/channel/java.rs:687 listen [inferred: one method named push]
+github.com/claudiomascaro/meada src/channel/java.rs:702 listen [inferred: one method named push]
+github.com/claudiomascaro/meada src/channel/java.rs:729 tests.sides [inferred: one method named push]
+github.com/claudiomascaro/meada src/channel/java.rs:745 tests.kt [inferred: one method named push]
+github.com/claudiomascaro/meada src/community.rs:143 leiden [inferred: one method named push]
+github.com/claudiomascaro/meada src/decider/mod.rs:256 setting_on_disk [inferred: one method named push]
+github.com/claudiomascaro/meada src/decider/mod.rs:481 run [inferred: one method named push]
+github.com/claudiomascaro/meada src/decisions.rs:345 replay [inferred: one method named push]
+github.com/claudiomascaro/meada src/decisions.rs:346 replay [inferred: one method named push]
+github.com/claudiomascaro/meada src/document/asyncapi.rs:72 read [inferred: one method named push]
+github.com/claudiomascaro/meada src/document/yaml.rs:113 Builder.place [inferred: one method named push]
+github.com/claudiomascaro/meada src/dotnet/msbuild.rs:314 layer [inferred: one method named push]
+github.com/claudiomascaro/meada src/engine.rs:545 Engine.search [inferred: one method named push]
+github.com/claudiomascaro/meada src/engine.rs:602 Engine.search [inferred: one method named push]
+github.com/claudiomascaro/meada src/engine.rs:727 Engine.search_duplicates [inferred: one method named push]
+github.com/claudiomascaro/meada src/engine.rs:854 Engine.contract_sides [inferred: one method named push]
+github.com/claudiomascaro/meada src/entry.rs:270 tests.found [inferred: one method named push]
+github.com/claudiomascaro/meada src/entry.rs:302 tests.export_doors [inferred: one method named push]
+github.com/claudiomascaro/meada src/http.rs:344 based [inferred: one method named push]
+github.com/claudiomascaro/meada src/http.rs:648 Http.ContractKind.declare [inferred: one method named push]
+github.com/claudiomascaro/meada src/http.rs:688 Http.ContractKind.declare [inferred: one method named push]
+github.com/claudiomascaro/meada src/http/csharp.rs:188 providers [inferred: one method named push]
+github.com/claudiomascaro/meada src/http/csharp/clients.rs:388 registered [inferred: one method named push]
+github.com/claudiomascaro/meada src/http/csharp/clients.rs:407 Index.of [inferred: one method named push]
+github.com/claudiomascaro/meada src/http/csharp/clients.rs:412 Index.of [inferred: one method named push]
+github.com/claudiomascaro/meada src/http/csharp/clients.rs:442 registration [inferred: one method named push]
+github.com/claudiomascaro/meada src/http/csharp/clients.rs:638 tests.sent_in [inferred: one method named push]
+github.com/claudiomascaro/meada src/http/csharp/refit.rs:85 consumers [inferred: one method named push]
+github.com/claudiomascaro/meada src/http/csharp/refit.rs:170 bases [inferred: one method named push]
+github.com/claudiomascaro/meada src/http/csharp/refit.rs:201 inherited [inferred: one method named push]
+github.com/claudiomascaro/meada src/http/csharp/refit.rs:228 url [inferred: one method named push]
+github.com/claudiomascaro/meada src/http/go.rs:585 Go.chains [inferred: one method named push]
+github.com/claudiomascaro/meada src/http/go.rs:795 providers [inferred: one method named push]
+github.com/claudiomascaro/meada src/http/go/clients.rs:176 Clients.reaches [inferred: one method named push]
+github.com/claudiomascaro/meada src/http/java.rs:386 tests.served_with [inferred: one method named push]
+github.com/claudiomascaro/meada src/http/java.rs:620 tests.served_kotlin [inferred: one method named push]
+github.com/claudiomascaro/meada src/http/java/clients.rs:308 base [inferred: one method named push]
+github.com/claudiomascaro/meada src/http/java/clients.rs:473 tests.sent_in [inferred: one method named push]
+github.com/claudiomascaro/meada src/http/java/declarative.rs:361 retrofit_clients [inferred: one method named push]
+github.com/claudiomascaro/meada src/http/java/declarative.rs:517 tests.sent_in [inferred: one method named push]
+github.com/claudiomascaro/meada src/http/kotlin.rs:192 Ktor.argument [inferred: one method named push]
+github.com/claudiomascaro/meada src/http/kotlin.rs:232 Ktor.scope_of [inferred: one method named push]
+github.com/claudiomascaro/meada src/http/kotlin.rs:274 Ktor.read [inferred: one method named push]
+github.com/claudiomascaro/meada src/http/kotlin.rs:303 Ktor.read [inferred: one method named push]
+github.com/claudiomascaro/meada src/http/kotlin.rs:316 Ktor.read [inferred: one method named push]
+github.com/claudiomascaro/meada src/http/kotlin.rs:359 Ktor.read [inferred: one method named push]
+github.com/claudiomascaro/meada src/http/kotlin/client.rs:86 Clients.reach [inferred: one method named push]
```
MCP stdio: 7/8 answers byte-equal; queries in /home/user/meada/.prove-it/runs/20261103T051214Z-audit-M2/probe/queries/meada.jsonl

## MCP HTTP vs stdio (head)

  meada: tools same, 8/8 answers byte-equal to stdio

