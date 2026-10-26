package example.util

object Tools {
    fun single(): Int = doubled()

    fun doubled(): Int = 2
}

fun tool(): Int = Tools.single()

fun tool(a: Int, b: Int): Int = a + b

internal fun internalTool(): Int = 0

val greet: () -> Int = { 0 }
