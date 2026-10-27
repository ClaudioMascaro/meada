public struct Greeter {
    public let name: String

    public init(name: String) {
        self.name = name
    }

    public func hello(to other: String, loudly: Bool = false) -> String {
        loudly ? "HELLO \(other)" : "hello \(other)"
    }

    func secret() -> String {
        "secret"
    }
}

public func greet(name: String) -> String {
    Greeter(name: name).hello(to: name)
}

public func greet(name: String, greeting: String) -> String {
    "\(greeting) \(name)"
}

public func greet(_ name: String) -> String {
    greet(name: name)
}

func internalGreet(name: String) -> String {
    name
}

public func record(level: Int, items: Any...) {
    print(level, items)
}
