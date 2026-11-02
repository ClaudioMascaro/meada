import Lib
import SwiftUI

typealias Again = StoreOf<Int>
typealias A = B
typealias B = A
typealias Anim = Animation

struct Feature {
    var store: StoreOf<Int>
    func tap() { _ = store.send(1) }
}

typealias Anim2 = Anim
typealias Mover = Movement

extension Animation {
    func spin() {}
    static func build() -> Int { 0 }
}
