package example.ktor.orders

import io.ktor.server.application.call
import io.ktor.server.response.respondText
import io.ktor.server.routing.Route
import io.ktor.server.routing.get
import io.ktor.server.routing.patch
import io.ktor.server.routing.route

// A routing extension function in another file, called under two prefixes.
fun Route.orderRoutes() {
    route("/orders") {
        get {
            call.respondText(listing())
        }
        patch("/{id?}") {
            call.respondText("")
        }
    }
}

fun listing(): String = "orders"
