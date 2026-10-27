extension Order {
    public init(json: String) {
        self.init(id: json)
    }

    public func price(quantity: Int) -> Int {
        return discount(quantity: quantity)
    }

    func discount(quantity: Int) -> Int {
        return self.price(quantity: quantity - 1)
    }
}

extension Order.Line {
    func doubled() -> Int {
        return total() * 2
    }
}
