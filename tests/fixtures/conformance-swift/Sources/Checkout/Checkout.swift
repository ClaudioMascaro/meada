import Foundation
import Store

struct Checkout {
    let service = OrderService(retries: 2)

    func run() {
        let order = Order(id: "1")
        _ = Order(json: "{}")
        _ = service.place(order: order)
        _ = format(order: order)
        _ = format(id: "1")
        _ = Store.format(order: order)
        summarize()
        missing()
    }
}
