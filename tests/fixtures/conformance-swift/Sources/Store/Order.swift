public struct Order {
    public let id: String

    public init(id: String) {
        self.id = id
    }

    public struct Line {
        public func total() -> Int {
            return 0
        }
    }
}

public func format(order: Order) -> String {
    return order.id
}

func format(id: String) -> String {
    return id
}

func log(_ message: String) {}
