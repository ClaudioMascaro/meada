import Lib

func use(_ typed: Store<Int, Int>, _ aliased: StoreOf<Int>, _ maybe: StoreOf<Int>?, _ again: Again) {
    _ = typed.send(1); _ = aliased.send(1)
    _ = maybe?.send(1); _ = again.send(1)
    _ = Reduce<Int, Int> { _, _ in 0 }
}

func cycle(_ a: A, _ anim: Anim) {
    a.spin()
    anim.spin()
}

final class Sender {
    func send(_ a: Int, animation: Int? = nil) {}
    func send(_ a: Int, assert: ((Int) -> Void)? = nil) {}
    func on(_ a: Int, h: Handler? = nil) {}
    func on(_ a: Int, n: Int? = nil) {}
}

func run<F>(_ f: F) {}

func trailing(_ x: Sender) {
    x.send(1) { _ in }
    x.send(1)
    x.on(1) { _ in }
    run { }
}

// A name from outside the repository may be an alias of a function type (issue #456).
final class Loader {
    func load(completion: @escaping Ext.CompletionHandler) {}
    func load(completion: @escaping (Int) -> Void) {}
}

// A type of the repository named like the outside alias's last segment does not read it.
struct Holder { struct CompletionHandler {} }

final class Lone { func fetch(completion: Ext.CompletionHandler) {} }

final class Pair {
    func put(_ a: Int, wheel: Wheel? = nil) {}
    func put(_ a: Int, then: (() -> Void)? = nil) {}
}

func outside(_ x: Loader, _ y: Lone, _ z: Pair) {
    x.load { _ in }
    y.fetch { }
    z.put(1) { }
}

func hidden(_ x: Hidden) { _ = x.send(1) }

final class Wheel { func spin() {} }

func local() {
    let x: StoreOf<Int> = make()
    _ = x.send(1)
}

func generic<StoreOf>(_ x: StoreOf) { _ = x.send(1) }

final class Other { func ping() -> Int { 0 } }

struct Box {
    typealias Store = Other
    func g(_ s: Store) { _ = s.ping() }
}

func untyped() {
    let w = make()
    w.spin()
}

func reach(_ chained: Anim2, _ mover: Mover) {
    chained.spin()
    mover.spin()
    _ = Anim.build()
}
