# prove.py run

differences found: 2

## meada

meada 1.1.0 (base) vs meada 1.1.0 (head)

Determinism: cold twice equal base True, head True; refresh equal base True, head True; content refresh equal base False, head False (edit meada-core/src/authorize.rs: Change -> ChangQ); renamed symbol in the warm export base True, head True; restore equal base False, head False; original symbol back base True, head True  NOT DETERMINISTIC
  base warm vs cold (edited tree):
    calls: +0 -0 ~3
      level: exact -> None: 1
        github.com/claudiomascaro/meada::tests/fixtures/conformance-swift-app/Sources/Page+Show.swift::Page.build()::Item#0
      level: exact -> inferred: 2
        github.com/claudiomascaro/meada::tests/fixtures/conformance-swift-app/Sources/Page+Show.swift::Page.show(_:)::item.label#0
        github.com/claudiomascaro/meada::tests/fixtures/conformance-swift-app/Sources/Scopes.swift::Shelf.stock(_:)::item.label#0
      target: set -> None: 1
        github.com/claudiomascaro/meada::tests/fixtures/conformance-swift-app/Sources/Page+Show.swift::Page.build()::Item#0
    communities: +1 -1
  base original vs restored:
    calls: +0 -0 ~3
      level: None -> exact: 1
        github.com/claudiomascaro/meada::tests/fixtures/conformance-swift-app/Sources/Page+Show.swift::Page.build()::Item#0
      level: inferred -> exact: 2
        github.com/claudiomascaro/meada::tests/fixtures/conformance-swift-app/Sources/Page+Show.swift::Page.show(_:)::item.label#0
        github.com/claudiomascaro/meada::tests/fixtures/conformance-swift-app/Sources/Scopes.swift::Shelf.stock(_:)::item.label#0
      target: None -> set: 1
        github.com/claudiomascaro/meada::tests/fixtures/conformance-swift-app/Sources/Page+Show.swift::Page.build()::Item#0
    communities: +1 -1
  head warm vs cold (edited tree):
    calls: +0 -0 ~3
      level: exact -> None: 1
        github.com/claudiomascaro/meada::tests/fixtures/conformance-swift-app/Sources/Page+Show.swift::Page.build()::Item#0
      level: exact -> inferred: 2
        github.com/claudiomascaro/meada::tests/fixtures/conformance-swift-app/Sources/Page+Show.swift::Page.show(_:)::item.label#0
        github.com/claudiomascaro/meada::tests/fixtures/conformance-swift-app/Sources/Scopes.swift::Shelf.stock(_:)::item.label#0
      target: set -> None: 1
        github.com/claudiomascaro/meada::tests/fixtures/conformance-swift-app/Sources/Page+Show.swift::Page.build()::Item#0
    communities: +1 -1
  head original vs restored:
    calls: +0 -0 ~3
      level: None -> exact: 1
        github.com/claudiomascaro/meada::tests/fixtures/conformance-swift-app/Sources/Page+Show.swift::Page.build()::Item#0
      level: inferred -> exact: 2
        github.com/claudiomascaro/meada::tests/fixtures/conformance-swift-app/Sources/Page+Show.swift::Page.show(_:)::item.label#0
        github.com/claudiomascaro/meada::tests/fixtures/conformance-swift-app/Sources/Scopes.swift::Shelf.stock(_:)::item.label#0
      target: None -> set: 1
        github.com/claudiomascaro/meada::tests/fixtures/conformance-swift-app/Sources/Page+Show.swift::Page.build()::Item#0
    communities: +1 -1
Budget:
  index seconds: 45.054 -> 43.895 (limit x1.25)
  peak RSS KiB: 236536 -> 237008 (limit x1.25)
  export bytes: 28672345 -> 28672345 (limit x1.5)
Export, base -> head:
  identical
MCP stdio: 16/16 answers byte-equal; queries in /home/user/meada/.prove-it/runs/20261103T120501Z-recheck2-final/queries/meada.jsonl

## MCP HTTP vs stdio (head)

  meada: tools same, 16/16 answers byte-equal to stdio

