package dev.kt.core

fun greet(name: String): String = "hello " + name

fun greet(name: String, times: Int): String = (1..times).joinToString(" ") { greet(name) }

fun String.shout(): String = uppercase() + "!"

fun probe(): String = "lib"

internal fun hidden(): String = "internal"

// A default argument lets a call leave it out; a vararg takes any number.
fun pick(x: Int, y: Int = 0): Int = x + y

fun pick(text: String): Int = text.length

fun many(vararg names: String): Int = names.size
