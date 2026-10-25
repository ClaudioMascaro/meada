package example

// A top-level `main` starts a program.
fun main(args: Array<String>) {
    Runner().main()
}

// A `main` member of a class or object does not.
class Runner {
    fun main() {}
}
