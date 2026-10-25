package example.ktor.fake

// Functions named like Ktor's, of the repository: no route.
fun routing(block: () -> Unit) = block()

fun options(path: String, block: () -> Unit) = block()

fun fake() {
    routing {
        options("/fake") { }
    }
}
