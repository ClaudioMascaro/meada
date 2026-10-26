package dev.shop

import dev.kt.core.*

fun bulk(): String = greet("all") + Money.of(2)

fun loudly(name: String): String = name.shout()
