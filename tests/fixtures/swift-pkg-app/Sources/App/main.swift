import Foundation
import Kit
import KitMoney
import Shared

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
