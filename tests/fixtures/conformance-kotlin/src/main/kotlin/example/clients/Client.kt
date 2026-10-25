package example.clients

import example.clients.urls.ORDERS
import example.clients.urls.Paths
import example.clients.urls.USERS
import io.ktor.client.HttpClient
import io.ktor.client.engine.cio.CIO
import io.ktor.client.plugins.defaultRequest
import io.ktor.client.request.*
import io.ktor.http.HttpMethod

val orders = HttpClient(CIO)

// Every verb, and `request` with the method its block sets (`GET` when it sets none).
suspend fun verbs() {
    orders.get("$ORDERS/orders")
    orders.post("$ORDERS/orders")
    orders.put("$ORDERS/orders/1")
    orders.patch("$ORDERS/orders/1")
    orders.delete("$ORDERS/orders/1")
    orders.head("$ORDERS/orders/head")
    orders.options("$ORDERS/orders/options")
    orders.request("$ORDERS/orders/request") { method = HttpMethod.Put }
    orders.request("$ORDERS/orders/default")
}

// The base of a `defaultRequest` joins the paths of the client's calls.
val billing = HttpClient(CIO) {
    defaultRequest {
        url("https://billing.example.com/api")
    }
}

val versioned = HttpClient(CIO) {
    defaultRequest {
        url("$ORDERS/v2")
    }
}

suspend fun based() {
    billing.get("/invoices")
    billing.get("https://other.example.com/absolute")
    versioned.post("/things")
}

// Strings, templates, `+` and constants fold; a hole that does not fold is a parameter.
suspend fun folds() {
    orders.get(USERS)
    orders.get(USERS + "/me")
    orders.get(ORDERS + Paths.ITEMS)
    orders.get(ORDERS + Paths.STOCK)
    orders.get("$ORDERS/users/${count()}/profile")
}

fun count(): Int = 3

// A path Meada cannot evaluate: an unresolved consumer.
suspend fun lookup() {
    orders.get(System.getProperty("orders.url"))
}

suspend fun reassigned() {
    orders.get(Paths.drifting)
}

// `url { path(..) }` builders are not read.
suspend fun builder() {
    orders.get {
        url { path("/hidden") }
    }
}

// A property of a class: the client its calls are made on.
class Gateway {
    private val client = HttpClient(CIO) {
        defaultRequest { url("https://gateway.example.com") }
    }

    suspend fun health() = client.get("/health")
}
