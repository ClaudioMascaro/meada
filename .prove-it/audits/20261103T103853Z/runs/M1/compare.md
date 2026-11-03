# prove.py run

differences found: 4

## meada

meada 1.1.0 (base) vs meada 1.1.0 (head)

Determinism: cold twice equal base True, head True; refresh equal base True, head True; content refresh equal base False, head False (edit meada-core/src/authorize.rs: Change -> ChangQ); restore equal base False, head False  NOT DETERMINISTIC
Budget:
  index seconds: 40.155 -> 42.496 (limit x1.25)
  peak RSS KiB: 236896 -> 236232 (limit x1.25)
  export bytes: 28672668 -> 28672668 (limit x1.5)
Export, base -> head:
  identical
MCP search {"query": "kind", "kind": "contract"}: differs
```
--- base
+++ head
@@ -1 +1,99 @@
-no matches for `kind`
+github.com/claudiomascaro/meada meada-core/src/manifest.rs:157 kind
+github.com/claudiomascaro/meada meada-core/src/msg.rs:40 Kind
+github.com/claudiomascaro/meada src/decisions.rs:46 Kind
+github.com/claudiomascaro/meada src/http/rust.rs:207 Web.kind
+github.com/claudiomascaro/meada src/lang/mod.rs:790 Kind
+github.com/claudiomascaro/meada meada-core/src/msg.rs:51 Kind.words
+github.com/claudiomascaro/meada src/contract.rs:360 kind_of
+github.com/claudiomascaro/meada src/decisions.rs:55 Kind.of
+github.com/claudiomascaro/meada src/engine.rs:1600 kind_words
+github.com/claudiomascaro/meada meada-core/src/export.rs:455 EntryKind
+github.com/claudiomascaro/meada meada-core/src/export.rs:476 EntryKind.name
+github.com/claudiomascaro/meada meada-core/src/export.rs:485 EntryKind.parse
+github.com/claudiomascaro/meada meada-core/src/manifest.rs:576 expected_kind
+github.com/claudiomascaro/meada src/channel.rs:309 Channel.ContractKind.at_call
+github.com/claudiomascaro/meada src/channel.rs:262 Channel.ContractKind.declare
+github.com/claudiomascaro/meada src/channel.rs:356 Channel.ContractKind.entry
+github.com/claudiomascaro/meada src/channel.rs:184 Channel.ContractKind.extract
+github.com/claudiomascaro/meada src/channel.rs:327 Channel.ContractKind.link
+github.com/claudiomascaro/meada src/channel.rs:180 Channel.ContractKind.parts
+github.com/claudiomascaro/meada src/channel.rs:112 Channel.ContractKind.prefix
+github.com/claudiomascaro/meada src/channel.rs:122 Channel.ContractKind.related_sides
+github.com/claudiomascaro/meada src/channel.rs:116 Channel.ContractKind.sides
+github.com/claudiomascaro/meada src/channel.rs:176 Channel.ContractKind.unresolved
+github.com/claudiomascaro/meada src/contract.rs:172 ContractKind
+github.com/claudiomascaro/meada src/contract.rs:222 ContractKind.after_links
+github.com/claudiomascaro/meada src/contract.rs:303 ContractKind.at_call
+github.com/claudiomascaro/meada src/contract.rs:227 ContractKind.calls_from_definition
+github.com/claudiomascaro/meada src/contract.rs:351 ContractKind.candidate
+github.com/claudiomascaro/meada src/contract.rs:323 ContractKind.context_reads
+github.com/claudiomascaro/meada src/contract.rs:310 ContractKind.context_sections
+github.com/claudiomascaro/meada src/contract.rs:340 ContractKind.decidable
+github.com/claudiomascaro/meada src/contract.rs:193 ContractKind.declare
+github.com/claudiomascaro/meada src/contract.rs:185 ContractKind.entry
+github.com/claudiomascaro/meada src/contract.rs:180 ContractKind.extract
+github.com/claudiomascaro/meada src/contract.rs:240 ContractKind.label
+github.com/claudiomascaro/meada src/contract.rs:212 ContractKind.link
+github.com/claudiomascaro/meada src/contract.rs:266 ContractKind.own_search
+github.com/claudiomascaro/meada src/contract.rs:246 ContractKind.parts
+github.com/claudiomascaro/meada src/contract.rs:253 ContractKind.place
+github.com/claudiomascaro/meada src/contract.rs:174 ContractKind.prefix
+github.com/claudiomascaro/meada src/contract.rs:290 ContractKind.related_sides
+github.com/claudiomascaro/meada src/contract.rs:260 ContractKind.search_word
+github.com/claudiomascaro/meada src/contract.rs:346 ContractKind.serves
+github.com/claudiomascaro/meada src/contract.rs:272 ContractKind.sides
+github.com/claudiomascaro/meada src/contract.rs:329 ContractKind.starts
+github.com/claudiomascaro/meada src/contract.rs:335 ContractKind.status
+github.com/claudiomascaro/meada src/contract.rs:199 ContractKind.symbols_read
+github.com/claudiomascaro/meada src/contract.rs:234 ContractKind.unresolved
+github.com/claudiomascaro/meada src/contract.rs:278 ContractKind.view
+github.com/claudiomascaro/meada src/contract.rs:206 ContractKind.workspace_step
+github.com/claudiomascaro/meada src/contract.rs:634 tests.a_kind_with_nothing_to_add_leaves_the_sides_the_call_and_the_symbol_alone
+github.com/claudiomascaro/meada src/contract.rs:612 tests.a_kind_with_sides_is_viewed_by_both_and_one_without_has_no_view
+github.com/claudiomascaro/meada src/contract.rs:556 tests.resources_report_status_lines_and_other_kinds_none
+github.com/claudiomascaro/meada src/contract.rs:514 tests.shown_and_typed_round_trip_through_the_kind
+github.com/claudiomascaro/meada src/contract.rs:593 tests.the_kinds_search_words_follow_the_kinds_order
+github.com/claudiomascaro/meada src/document/asyncapi.rs:365 tests.the_server_protocol_gives_the_broker_kind
```
MCP search {"query": "zz_prove_missing", "kind": "zz_prove_kind"}: differs
```
--- base
+++ head
@@ -1 +1 @@
-TOOL ERROR `kind` takes `symbol`, `contract`, `route`, `resource`, `channel`, `document`, `package`, `drift`, `community`, `process`, `duplicate`, `ambiguous` or `unresolved`, not `zz_prove_kind`
+no matches for `zz_prove_missing`
```
MCP stdio: 14/16 answers byte-equal; queries in /home/user/meada/.prove-it/runs/20261103T110241Z-recheck-M1/queries/meada.jsonl

## MCP HTTP vs stdio (head)

  meada: tools same, 16/16 answers byte-equal to stdio

