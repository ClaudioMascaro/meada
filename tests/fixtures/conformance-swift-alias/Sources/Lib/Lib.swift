public struct Reduce<S, A> {
    init(internal reduce: @escaping (S, A) -> Int) {}
    public init(_ reduce: @escaping (S, A) -> Int) { self.init(internal: reduce) }
    public init(_ reducer: some Reducer) {}
}

public protocol Reducer {}

public final class Store<S, A> { public func send(_ action: A) -> Int { 0 } }

public typealias StoreOf<S> = Store<S, Int>
public typealias Handler = (Int) -> Void

func inside() {
    _ = Reduce<Int, Int>(internal: { _, _ in 0 })
}
typealias Hidden = Store<Int, Int>
