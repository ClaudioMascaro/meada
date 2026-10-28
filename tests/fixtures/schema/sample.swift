import Store
@testable import Kit

public struct Order {
    public init(id: String) {}
}

extension Order: Codable {
    func place(order o: Order, quantity: Int = 1, _ rest: Int...) {
        audit(o)
        retry { }
    }
}

let limit = 10

public typealias Handler = (Int) -> Void
typealias Orders = Order

func on(_ handler: Handler? = nil) {}

func settle() async {
    await flush()
}

fileprivate func local() {}

func shared() {
    let session = URLSession.shared
    session.finishTasksAndInvalidate()
}
