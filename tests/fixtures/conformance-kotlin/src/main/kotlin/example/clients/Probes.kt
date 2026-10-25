package example.clients

import io.ktor.client.HttpClient
import io.ktor.client.request.*

// A client nobody passes one to is a client by its written type: no base.
suspend fun typed(client: HttpClient) = client.get("https://orders.example.com/typed")

// A parameter and a local hide the client of the same name.
fun hiddenByParameter(orders: Map<String, String>) {
    orders.get("https://orders.example.com/param")
}

fun hiddenByLocal() {
    val orders = mapOf("a" to "b")
    orders.get("https://orders.example.com/local")
}

// A client of another library whose name Ktor's `get` shares.
fun otherLibrary() {
    val http = java.net.http.HttpClient.newHttpClient()
    http.get("https://orders.example.com/other")
}
