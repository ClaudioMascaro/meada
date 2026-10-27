extension Order {
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
