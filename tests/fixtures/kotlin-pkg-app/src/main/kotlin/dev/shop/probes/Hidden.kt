package dev.shop.probes

import dev.kt.core.greet

// A name the code binds itself hides the imported one.
fun localFun(): String {
    fun greet(text: String): String = text
    return greet("local")
}

fun parameter(greet: (String) -> String): String = greet("p")

fun localValue(): String {
    val greet = { text: String -> text }
    return greet("v")
}

class Holder {
    fun greet(text: String): String = text

    fun use(): String = greet("member")
}

fun real(): String = greet("imported")
