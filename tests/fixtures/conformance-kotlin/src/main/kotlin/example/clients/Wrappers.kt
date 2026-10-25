package example.clients

import example.clients.urls.ORDERS
import io.ktor.client.HttpClient
import io.ktor.client.request.get

// A wrapper whose URL depends on its parameters: each call site is a consumer, through any
// number of levels.
suspend fun fetch(client: HttpClient, path: String) = client.get("$ORDERS$path")

suspend fun order(id: String) = fetch(orders, "/orders/$id")

suspend fun report() {
    order("a1")
    order("b2")
}

// A cycle adds no consumer of its own.
suspend fun ping(path: String): Any = pong(path)

suspend fun pong(path: String): Any {
    orders.get("$ORDERS$path")
    return ping(path)
}

suspend fun start() {
    ping("/loop")
}

// Nobody calls it: its parameter is a hole.
suspend fun user(id: Int) = orders.get("$ORDERS/users/$id")
