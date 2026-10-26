package dev.shop

import dev.kt.core.Box
import dev.kt.core.Money
import dev.kt.core.label
import dev.kt.core.many
import dev.kt.core.pick
import dev.kt.core.render
import dev.kt.core.shout
import dev.kt.core.total
import dev.kt.other.*
import outside.absent.Thing

// A call that leaves a default argument out may reach it: the overloads are told apart by what
// the calls could be, never by the count alone.
fun defaults(): Int = pick(1)

fun every(): Int = pick(1, 2)

fun varargs(): Int = many("a", "b")

// An extension of a class of the provider, and of a type from outside the workspace, in a file
// of neither: a member of the receiver's type with the name would win.
fun labelled(money: Money): String = money.label()

fun drawn(thing: Thing): String = thing.render()

fun counted(money: Money): Int = money.total()

// An explicit import beats a star import of the same name.
fun shouted(text: String): String = text.shout()

// A call on the class's name is the companion's, one on an instance the member's.
fun viaClass(): Box = Box.of(1)

fun viaInstance(box: Box): Box = box.of(2)
