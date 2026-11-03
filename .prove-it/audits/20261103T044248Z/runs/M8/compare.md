# prove.py run

differences found: 5

## meada

meada 1.1.0 (base) vs meada 1.1.0 (head)

Determinism: cold twice equal base True, head True; refresh equal base True, head True
Budget:
  index seconds: 45.171 -> 46.846 (limit x1.25)
  peak RSS KiB: 236120 -> 236772 (limit x1.25)
  export bytes: 28672668 -> 28672668 (limit x1.5)
Export, base -> head:
  identical
MCP search {"query": "kind"}: differs
```
--- base
+++ head
@@ -97,3 +97,2 @@
 github.com/claudiomascaro/meada src/resource.rs:150 Resource.ContractKind.link
-github.com/claudiomascaro/meada src/resource.rs:92 Resource.ContractKind.parts
-… cut after 98 of 104 lines (budget 2000 tokens); raise budget for more
+… cut after 97 of 104 lines (budget 2000 tokens); raise budget for more
```
MCP context {"symbol": "github.com/claudiomascaro/meada::src/http/rust.rs::Web.kind"}: differs
```
--- base
+++ head
@@ -82,3 +82,2 @@
 github.com/claudiomascaro/meada src/lang/csharp.rs:1709 base_class [inferred: one method named kind]
-github.com/claudiomascaro/meada src/lang/csharp.rs:1740 type_spelling [inferred: one method named kind]
-… cut after 83 of 685 lines (budget 2000 tokens); raise budget for more
+… cut after 82 of 685 lines (budget 2000 tokens); raise budget for more
```
MCP trace {"from": "github.com/claudiomascaro/meada::src/http/rust.rs::Web.kind"}: differs
```
--- base
+++ head
@@ -72,4 +72,3 @@
                                           github.com/claudiomascaro/meada src/resolve.rs:2819 Resolver.lineage
-                                            github.com/claudiomascaro/meada src/trace.rs:712 Walk.push [inferred: one method named push] (see above)
-… cut after 73 of 9356 lines (budget 2000 tokens); raise budget for more
-chain: ambiguous · 0 hops · 1 repos · 1982 tokens
+… cut after 72 of 9356 lines (budget 2000 tokens); raise budget for more
+chain: ambiguous · 0 hops · 1 repos · 1944 tokens
```
MCP context {"symbol": "github.com/claudiomascaro/meada::src/trace.rs::Walk.push"}: differs
```
--- base
+++ head
@@ -78,3 +78,2 @@
 github.com/claudiomascaro/meada src/lang/csharp.rs:606 parse [inferred: one method named push]
-github.com/claudiomascaro/meada src/lang/csharp.rs:618 parse [inferred: one method named push]
-… cut after 79 of 414 lines (budget 2000 tokens); raise budget for more
+… cut after 78 of 414 lines (budget 2000 tokens); raise budget for more
```
MCP context {"symbol": "github.com/claudiomascaro/meada::src/lang/mod.rs::text"}: differs
```
--- base
+++ head
@@ -116,3 +116,2 @@
 github.com/claudiomascaro/meada src/lang/go/values.rs:282 fields
-github.com/claudiomascaro/meada src/lang/go/values.rs:397 keeps_env
-… cut after 117 of 433 lines (budget 2000 tokens); raise budget for more
+… cut after 116 of 433 lines (budget 2000 tokens); raise budget for more
```
MCP stdio: 6/11 answers byte-equal; queries in /home/user/meada/.prove-it/runs/20261103T052309Z-audit-M8/queries/meada.jsonl

## MCP HTTP vs stdio (head)

  meada: tools same, 11/11 answers byte-equal to stdio

