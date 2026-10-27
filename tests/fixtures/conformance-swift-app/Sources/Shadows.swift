func shadow() {}

func error() {}

enum Choice {
    case run(() -> Void)
}

func closures(_ list: [() -> Void]) {
    list.forEach { shadow in
        shadow()
    }
}

func optionals(_ f: (() -> Void)?) {
    if let shadow = f {
        shadow()
    }
}

func guarded(_ f: (() -> Void)?) {
    guard let shadow = f else { return }
    shadow()
}

func loops(_ fs: [() -> Void]) {
    for shadow in fs {
        shadow()
    }
}

func caught() {
    do {
        try work()
    } catch {
        error()
    }
}

func cases(_ choice: Choice) {
    switch choice {
    case let .run(shadow):
        shadow()
    }
}

func work() throws {}
