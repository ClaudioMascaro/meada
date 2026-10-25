package example.ktor.fake.typed

import io.ktor.server.application.Application
import io.ktor.server.resources.*
import io.ktor.server.routing.*

class Typed

// Type-safe routing (resource classes) is not read.
fun Application.typed() {
    routing {
        route("/typed") {
            get<Typed> { }
        }
    }
}

// Not in scope of `example.ktor`, which imports the other `orderRoutes`: mounted nowhere.
fun Route.orderRoutes() {
    get("/wrong") { }
}
