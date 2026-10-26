package dev.kt.core

class Money(val cents: Int) {
    fun plus(other: Money): Money = Money(cents + other.cents)

    fun total(): Int = cents

    companion object {
        fun of(cents: Int): Money = Money(cents)
    }
}

fun Money.format(): String = greet("$" + total())

fun Money.stamp(): String = "lib"
