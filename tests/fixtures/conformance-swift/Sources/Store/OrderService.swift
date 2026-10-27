import Foundation

public protocol Notifier {
    func notify(order: Order)
}

extension Notifier {
    public func notify(order: Order) {
        log(order.id)
    }

    public func notifyAll(orders: [Order]) {
        for order in orders {
            notify(order: order)
        }
    }
}

open class OrderService: Notifier {
    public init() {}

    public init(retries: Int) {
        self.init()
    }

    public func place(order: Order) -> Bool {
        return place(order: order, quantity: 1)
    }

    public func place(order: Order, quantity: Int) -> Bool {
        audit(order)
        self.notify(order: order)
        _ = order.price(quantity: quantity)
        return OrderService.validate(order: order)
    }

    public func place(_ id: String) -> Bool {
        return place(order: Order(id: id))
    }

    func audit(_ order: Order) {}

    func audit(_ id: String) {}

    static func validate(order: Order) -> Bool {
        return true
    }

    func retry(times: Int = 3, _ body: () -> Void) {}

    func run() {
        retry {
            log("retry")
        }
        retry(times: 2) {}
    }
}

public final class Reporter: NSObject {
    func report() {
        summary()
    }

    func summary() {}
}
