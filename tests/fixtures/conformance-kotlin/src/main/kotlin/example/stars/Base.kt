package example.stars

open class Base {
    fun mark(n: Int) {}

    fun runIt(x: Int = 1, block: () -> Unit) {}
}

interface Face {
    fun face() {}
}
