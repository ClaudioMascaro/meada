package example.clients.fake

import io.ktor.client.request.*

// Not Ktor's: this `HttpClient` is the repository's, and a map is no client.
class HttpClient {
    fun options(url: String) = url
}

fun fake(client: HttpClient, map: Map<String, String>) {
    client.options("https://orders.example.com/fake")
    map.get("https://orders.example.com/map")
}
