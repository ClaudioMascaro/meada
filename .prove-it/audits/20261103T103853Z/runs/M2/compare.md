# prove.py run

differences found: 3

## meada

meada 1.1.0 (base) vs meada 1.1.0 (head)

Determinism: cold twice equal base True, head True; refresh equal base True, head True; content refresh equal base False, head False (edit meada-core/src/authorize.rs: Change -> ChangQ); restore equal base False, head False  NOT DETERMINISTIC
Budget:
  index seconds: 41.641 -> 42.054 (limit x1.25)
  peak RSS KiB: 236320 -> 236872 (limit x1.25)
  export bytes: 28672668 -> 28672668 (limit x1.5)
Export, base -> head:
  identical
MCP context {"symbol": "github.com/claudiomascaro/meada::src/http/rust.rs::Web.kind", "budget": 300}: differs
```
--- base
+++ head
@@ -13,2 +13,72 @@
 github.com/claudiomascaro/meada src/lang/csharp.rs:500 parse [inferred: one method named kind]
-… cut after 13 of 685 lines (budget 300 tokens); raise budget for more
+github.com/claudiomascaro/meada src/lang/csharp.rs:604 parse [inferred: one method named kind]
+github.com/claudiomascaro/meada src/lang/csharp.rs:611 parse [inferred: one method named kind]
+github.com/claudiomascaro/meada src/lang/csharp.rs:653 parse [inferred: one method named kind]
+github.com/claudiomascaro/meada src/lang/csharp.rs:668 parse [inferred: one method named kind]
+github.com/claudiomascaro/meada src/lang/csharp.rs:693 parse [inferred: one method named kind]
+github.com/claudiomascaro/meada src/lang/csharp.rs:698 parse [inferred: one method named kind]
+github.com/claudiomascaro/meada src/lang/csharp.rs:699 parse [inferred: one method named kind]
+github.com/claudiomascaro/meada src/lang/csharp.rs:705 parse [inferred: one method named kind]
+github.com/claudiomascaro/meada src/lang/csharp.rs:709 parse [inferred: one method named kind]
+github.com/claudiomascaro/meada src/lang/csharp.rs:719 parse [inferred: one method named kind]
+github.com/claudiomascaro/meada src/lang/csharp.rs:727 parse [inferred: one method named kind]
+github.com/claudiomascaro/meada src/lang/csharp.rs:732 parse [inferred: one method named kind]
+github.com/claudiomascaro/meada src/lang/csharp.rs:755 parse [inferred: one method named kind]
+github.com/claudiomascaro/meada src/lang/csharp.rs:756 parse [inferred: one method named kind]
+github.com/claudiomascaro/meada src/lang/csharp.rs:758 parse [inferred: one method named kind]
+github.com/claudiomascaro/meada src/lang/csharp.rs:761 parse [inferred: one method named kind]
+github.com/claudiomascaro/meada src/lang/csharp.rs:765 parse [inferred: one method named kind]
+github.com/claudiomascaro/meada src/lang/csharp.rs:775 parse [inferred: one method named kind]
+github.com/claudiomascaro/meada src/lang/csharp.rs:783 parse [inferred: one method named kind]
+github.com/claudiomascaro/meada src/lang/csharp.rs:792 parse [inferred: one method named kind]
+github.com/claudiomascaro/meada src/lang/csharp.rs:794 parse [inferred: one method named kind]
+github.com/claudiomascaro/meada src/lang/csharp.rs:796 parse [inferred: one method named kind]
+github.com/claudiomascaro/meada src/lang/csharp.rs:797 parse [inferred: one method named kind]
+github.com/claudiomascaro/meada src/lang/csharp.rs:807 parse [inferred: one method named kind]
+github.com/claudiomascaro/meada src/lang/csharp.rs:850 parse [inferred: one method named kind]
+github.com/claudiomascaro/meada src/lang/csharp.rs:854 parse [inferred: one method named kind]
+github.com/claudiomascaro/meada src/lang/csharp.rs:861 parse [inferred: one method named kind]
+github.com/claudiomascaro/meada src/lang/csharp.rs:864 parse [inferred: one method named kind]
+github.com/claudiomascaro/meada src/lang/csharp.rs:925 parse [inferred: one method named kind]
+github.com/claudiomascaro/meada src/lang/csharp.rs:930 parse [inferred: one method named kind]
+github.com/claudiomascaro/meada src/lang/csharp.rs:954 parse [inferred: one method named kind]
+github.com/claudiomascaro/meada src/lang/csharp.rs:970 parse [inferred: one method named kind]
+github.com/claudiomascaro/meada src/lang/csharp.rs:974 parse [inferred: one method named kind]
+github.com/claudiomascaro/meada src/lang/csharp.rs:995 parse [inferred: one method named kind]
+github.com/claudiomascaro/meada src/lang/csharp.rs:1028 parse [inferred: one method named kind]
+github.com/claudiomascaro/meada src/lang/csharp.rs:1035 parse [inferred: one method named kind]
+github.com/claudiomascaro/meada src/lang/csharp.rs:1036 parse [inferred: one method named kind]
+github.com/claudiomascaro/meada src/lang/csharp.rs:1039 parse [inferred: one method named kind]
+github.com/claudiomascaro/meada src/lang/csharp.rs:1071 parse [inferred: one method named kind]
+github.com/claudiomascaro/meada src/lang/csharp.rs:1115 parse [inferred: one method named kind]
+github.com/claudiomascaro/meada src/lang/csharp.rs:1168 parse [inferred: one method named kind]
+github.com/claudiomascaro/meada src/lang/csharp.rs:1169 parse [inferred: one method named kind]
+github.com/claudiomascaro/meada src/lang/csharp.rs:1188 parse [inferred: one method named kind]
+github.com/claudiomascaro/meada src/lang/csharp.rs:1205 parse [inferred: one method named kind]
+github.com/claudiomascaro/meada src/lang/csharp.rs:1208 parse [inferred: one method named kind]
+github.com/claudiomascaro/meada src/lang/csharp.rs:1214 parse [inferred: one method named kind]
+github.com/claudiomascaro/meada src/lang/csharp.rs:1426 declares_main [inferred: one method named kind]
+github.com/claudiomascaro/meada src/lang/csharp.rs:1427 declares_main [inferred: one method named kind]
+github.com/claudiomascaro/meada src/lang/csharp.rs:1428 declares_main [inferred: one method named kind]
+github.com/claudiomascaro/meada src/lang/csharp.rs:1445 lambdas [inferred: one method named kind]
+github.com/claudiomascaro/meada src/lang/csharp.rs:1517 using [inferred: one method named kind]
+github.com/claudiomascaro/meada src/lang/csharp.rs:1523 modifiers [inferred: one method named kind]
+github.com/claudiomascaro/meada src/lang/csharp.rs:1537 parameters_of [inferred: one method named kind]
+github.com/claudiomascaro/meada src/lang/csharp.rs:1540 parameters_of [inferred: one method named kind]
+github.com/claudiomascaro/meada src/lang/csharp.rs:1554 parameters_of [inferred: one method named kind]
```
MCP stdio: 15/16 answers byte-equal; queries in /home/user/meada/.prove-it/runs/20261103T111232Z-recheck-M2/queries/meada.jsonl

## MCP HTTP vs stdio (head)

  meada: tools same, 16/16 answers byte-equal to stdio

