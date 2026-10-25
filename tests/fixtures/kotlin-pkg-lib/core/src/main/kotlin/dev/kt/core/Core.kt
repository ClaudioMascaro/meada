package dev.kt.core

fun greet(name: String): String = "hello " + name

fun greet(name: String, times: Int): String = (1..times).joinToString(" ") { greet(name) }

fun String.shout(): String = uppercase() + "!"

fun probe(): String = "lib"

internal fun hidden(): String = "internal"
