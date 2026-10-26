package example

// A top-level `main` starts a program.
fun main(args: Array<String>) {
    Runner().main()
}

// A `main` member of a class or object does not.
class Runner {
    fun main() {}
}

object Launcher {
    fun main() {}
}

// An extension named `main` is no entry point either.
fun String.main() {}
