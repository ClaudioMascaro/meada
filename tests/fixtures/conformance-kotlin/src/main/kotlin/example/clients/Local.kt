package example.clients

import io.ktor.client.HttpClient
import io.ktor.client.engine.cio.CIO
import io.ktor.client.plugins.defaultRequest
import io.ktor.client.request.*

// A client a local holds, with no type written: the call that made it gives its base.
suspend fun untyped() {
    val client = HttpClient(CIO)
    client.get("https://local.example.com/untyped")
}

suspend fun closing() {
    HttpClient(CIO).use { it.get("https://local.example.com/use") }
}

suspend fun closingNamed() {
    HttpClient(CIO) { defaultRequest { url("https://local.example.com/named") } }
        .use { client -> client.get("/inner") }
}

// Two clients of one function, each with its own configuration.
suspend fun two() {
    val based = HttpClient(CIO) { defaultRequest { url("https://local.example.com/api") } }
    val plain = HttpClient(CIO)
    plain.get("https://local.example.com/plain")
    based.get("/items")
}

// A local named like a client of the file hides it: its own base, not `billing`'s.
suspend fun shadows() {
    val billing = HttpClient(CIO) { defaultRequest { url("https://shadow.example.com/v9") } }
    billing.get("/x")
}

// A `var` may hold another client by the time it is used: not read.
suspend fun variable() {
    var client = HttpClient(CIO)
    client = HttpClient(CIO) { defaultRequest { url("https://local.example.com/other") } }
    client.get("/var")
}
