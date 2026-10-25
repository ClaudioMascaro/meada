package example

// Each kind of class, nested names, an object, companions and an extension function.
data class Point(val x: Int, val y: Int) {
    fun moved(dx: Int): Point = Point(x + dx, y)

    // A class-qualified companion call and a bare one, both to the unnamed companion.
    fun twice(): Point = Point.origin().scaled(2)

    fun fromOrigin(): Point = origin()

    fun scaled(factor: Int): Point = Point(x * factor, y * factor)

    companion object {
        fun origin(): Point = Point(0, 0)

        fun origin(x: Int): Point = Point(x, 0)
    }
}

sealed class Shape {
    abstract fun area(): Int

    class Square(val side: Int) : Shape() {
        override fun area(): Int = side * side

        inner class Corner {
            fun area(): Int = 0
        }
    }
}

enum class Color { RED, GREEN }

annotation class Marker

interface Named {
    fun name(): String
}

class Box : Named {
    override fun name(): String = label()

    fun label(): String = "box"

    // `this.method()` and a named companion.
    fun relabel(): String = this.label() + Box.create().name()

    // Inside a lambda, `this` may be a receiver no syntax names: a name match.
    fun later(): () -> String = { this.label() }

    companion object Factory {
        fun create(): Box = Box()
    }
}

object Registry {
    fun find(key: String): Point? = null

    fun find(key: String, fallback: Point): Point = find(key) ?: fallback

    fun all(): List<Point> = emptyList()
}

fun Point.norm(): Int = x * x + y * y

fun String.shout(): String = uppercase()

// A top-level property with a getter is a definition; its calls belong to it.
val greeting: String
    get() = Registry.all().toString()

fun helper(): Int = Registry.all().size

fun helper(n: Int): Int = n

fun helper(n: String): Int = n.length

fun defaults(a: Int, b: Int = 0): Int = a + b

fun many(vararg ids: Int): Int = ids.size

fun calls() {
    helper()
    helper(1)
    defaults(1)
    defaults(1, 2)
    many(1, 2, 3)
    Registry.find("a")
    Registry.find("a", Point(0, 0))
    Point.origin()
    Point.origin(1)
    Box.create()
    Box.Factory.create()
    Shape.Square(2).area()
    listOf(1, 2).forEach { helper(it) }
    println(greeting)
}

internal fun inside(): Int = helper()

private fun secret(): Int = inside()

@Marker
fun open(): Int = secret()

// Parameters, loop variables, `catch` parameters and local functions hide outer names.
fun hiding(helper: (Int) -> Int, runs: List<() -> Int>): Int {
    helper(1)
    for (same in runs) same()
    try {
        many()
    } catch (secret: Exception) {
        secret.printStackTrace()
    }
    fun inside(): Int = 0
    return inside()
}
