package dev.shop.own

import dev.kt.core.Money

// The app's own extension: the provider's, never imported, is not the one called.
fun Money.stamp(): String = "own"

fun stamped(money: Money): String = money.stamp()
