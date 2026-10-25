package example.ktor

import io.ktor.server.application.Application
import io.ktor.server.routing.*

// A local `val` followed by a line that starts with `get` or `set` is a statement, never an
// accessor: kotlinc reads accessors on members and top-level properties only.
fun Application.locals() {
    routing {
        val service = Service()
        get("/local") {
            audit()
        }
        val shared by lazy { service }
        get {
            audit()
        }
    }
}

fun Application.stored() {
    val key = "k"
    set("k", key)
}

// `set` with a block is a call too, not a setter.
fun Application.configured() {
    val key = "k"
    set {
        audit()
    }
}

// `set(` with a lambda on the same line, with and without a space before the parenthesis.
fun Application.keyed() {
    val key = "k"
    set("k") { audit() }
}

fun Application.spaced() {
    val key = "k"
    set ("k") { audit() }
}

fun Application.spacedPair() {
    val key = "k"
    set ("k", key)
}

fun afterLocals() {}
