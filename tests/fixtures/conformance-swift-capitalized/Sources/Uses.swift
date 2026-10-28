import XCTest

struct Store<S, A> {
  init(initialState: S) {}
}

func g() throws -> Int { 1 }

func awaited() async {
  await XCTAssertNoThrow(try g())
}

func synchronous() {
  XCTAssertNoThrow(try g())
}

func types() {
  _ = Store<Int, Int>(initialState: 0)
  _ = Text("x")
}

func Load() async -> Int { 1 }

func spawn() async {
  async let a = Load()
  Task { await Load() }
  Task { Load() }
}
