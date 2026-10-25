package example.ktor.hidden

import io.ktor.server.application.Application
import io.ktor.server.routing.*

// A parameter named like a routing extension hides it: Kotlin calls the parameter.
fun Application.hidden(catalog: Route.() -> Unit) {
    routing {
        route("/hidden") { catalog() }
    }
}

// A local function in a block hides the top-level one; Meada reads neither.
fun Application.local() {
    routing {
        fun Route.users() {
            get("/inner-users") { }
        }
        route("/v2") { users() }
    }
}

fun Route.catalog() {
    get("/o") { }
}

fun Route.users() {
    get("/u") { }
}

// An extension on `Routing`, and one that calls itself in a block: the cycle adds no prefix.
fun Application.cycle() {
    routing {
        top()
        again()
        // A `route` block's receiver is a `Route`: `top` takes the `Routing` around it.
        route("/r") { top() }
    }
}

fun Routing.top() {
    get("/top") { }
}

fun Route.again() {
    route("/cyc") {
        again()
        get("/loop") { }
    }
}
