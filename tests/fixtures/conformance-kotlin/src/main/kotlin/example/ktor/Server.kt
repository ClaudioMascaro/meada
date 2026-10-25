package example.ktor

import example.ktor.orders.orderRoutes
import io.ktor.http.HttpMethod
import io.ktor.server.application.Application
import io.ktor.server.application.call
import io.ktor.server.response.respondText
import io.ktor.server.routing.*

const val V1 = "/v1"

// Ktor routes: each enclosing `route` block's prefix, through the routing extension functions
// called inside one.
fun Application.module() {
    routing {
        get("/health") {
            call.respondText("ok")
        }
        route("/api") {
            route(V1) {
                get {
                    call.respondText("v1")
                }
                post("/items/{id}") {
                    audit()
                }
                orderRoutes()
            }
            route("/admin") {
                orderRoutes()
                adminRoutes()
            }
            route("/ping", HttpMethod.Head) {
                handle {
                    call.respondText("")
                }
            }
        }
        // A prefix Meada cannot read mounts nothing.
        route(prefix()) {
            get("/unread") { }
        }
    }
}

fun prefix(): String = "/p"

// No routing scope calls it: served nowhere.
fun Route.orphan() {
    get("/orphan") { }
}

fun Route.adminRoutes() {
    delete("/cache") {
        audit()
    }
}

fun audit() {}
