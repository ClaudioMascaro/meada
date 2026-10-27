import Foundation
import Kit
import KitMoney
import Shared
import Dyn

func welcome(who: String) -> String {
    greet(name: who)
}

func formal(who: String) -> String {
    Kit.greet(name: who, greeting: "Dear")
}

func plain(who: String) -> String {
    greet(who)
}

func hello(who: String) -> String {
    let greeter = Greeter(name: who)
    return greeter.hello(to: "world")
}

func typed(greeter: Greeter) -> String {
    greeter.hello(to: "you", loudly: true)
}

func total() -> Int {
    Money.of(cents: 5).plus(Money(cents: 1)).cents
}

func loud(text: String) -> String {
    shout(text)
}

func local() {
    print(internalGreet(name: "x"))
}

func both(m: Money) -> Money {
    m.plus(m)
}

func variadic() {
    record(level: 1, items: "a", "b")
}

func viaDyn() -> String {
    dynamic(name: "x")
}

// Names the code declares itself hide the ones an import brings.
struct Local {
    func hello(to other: String) -> String { other }
}

func hidesGreet(greet: (String) -> String) -> String {
    greet("x")
}

func hidesGreetByLocal() -> String {
    let greet = { (name: String) in name }
    return greet("x")
}

func hidesGreetByClosure() -> [String] {
    ["a"].map { greet in greet }
}

func hidesGreeterByParameter(greeter: Local) -> String {
    greeter.hello(to: "x")
}

func hidesGreeterByLocal() -> String {
    let greeter = Local()
    return greeter.hello(to: "x")
}

func hidesTheTypeByParameter(Greeter: Local) -> String {
    Greeter.hello(to: "x")
}

func twoOrigins(flag: Bool) -> String {
    var held = Greeter(name: "x")
    if flag { held = Local() }
    return held.hello(to: "x")
}

func onAResult() -> String {
    let result = Greeter(name: "x").hello(to: "y")
    return result.hello(to: "x")
}
