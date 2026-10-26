package dev.shop

import dev.kt.core.Money
import dev.kt.core.Money as Cash
import dev.kt.core.checked
import dev.kt.core.extras
import dev.kt.core.format
import dev.kt.core.greet
import dev.kt.core.hidden
import dev.kt.core.loose
import dev.kt.core.sample
import dev.kt.core.shout
import dev.kt.shared.Stamp
import dev.kt.shared.stamped
import outside.absent.Thing

fun pay(quantity: Int): String {
    val order: Money = Money.of(quantity)
    val doubled: Money = order.plus(Money(quantity))
    return doubled.format() + order.total()
}

fun hello(): String = greet("shop") + greet("shop", 2)

// An extension is imported by its name, the receiver's type is not a type of the provider.
fun loud(name: String): String = name.shout()

fun aliased(): Cash = Cash.of(1)

fun stamp(): String = Stamp.now() + stamped()

// Neither is in a source set root of the provider: the package declaration decides.
fun reached(): Money = extras()

fun loosely(): Money = loose()

fun unreached(name: String): Money {
    // A function is no member of a receiver, and an internal one is no door.
    name.greet()
    hidden()
    Thing.run()
    checked()
    return sample()
}
