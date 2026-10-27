public struct Money {
    public let cents: Int

    public init(cents: Int) {
        self.cents = cents
    }

    public static func of(cents: Int) -> Money {
        Money(cents: cents)
    }

    public func plus(_ other: Money) -> Money {
        Money(cents: cents + other.cents)
    }
}
