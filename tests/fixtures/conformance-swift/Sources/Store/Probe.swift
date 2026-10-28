protocol Pinger {
    func ping()
}

extension Pinger {
    func ping() {}

    func trace() {}
}

protocol Tracer {}

struct Probe {
    func run() {
        ping()
        trace()
    }
}

extension Probe: Pinger {}

struct Bag {
    func run() {
        describe()
    }
}

extension Bag: CustomStringConvertible {}

func describe() {}

// An outside protocol the pack does not list may declare any name.
struct Sack {
    func run() {
        describe()
    }
}

extension Sack: CustomDebugStringConvertible {}
