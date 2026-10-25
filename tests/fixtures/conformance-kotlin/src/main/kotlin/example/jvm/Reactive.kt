package example.jvm

import org.springframework.http.HttpMethod
import org.springframework.web.reactive.function.client.WebClient

val ORDERS_CLIENT = WebClient.create("https://orders.example.com/api")
val BUILT_CLIENT = WebClient.builder().baseUrl("https://built.example.com").build()

// The base is the one of the client the request is made on, each method starts its own request.
class Reactive(private val injected: WebClient) {
    fun get() {
        ORDERS_CLIENT.get().uri("/reactive/{id}", 1).retrieve()
    }

    fun flag() {
        BUILT_CLIENT.method(HttpMethod.PATCH).uri("/reactive/1/flag").retrieve()
    }

    // A client the constructor takes has no base Meada knows.
    fun typed() {
        injected.post().uri("https://typed.example.com/reactive").retrieve()
    }

    // A local the source writes no type for holds the call that made it: its base is the client's.
    fun local() {
        val client = WebClient.create("https://local.example.com")
        client.delete().uri("/reactive/local").retrieve()
    }

    fun builtLocal() {
        val client = WebClient.builder().baseUrl("https://built-local.example.com").build()
        client.put().uri("/reactive/built").retrieve()
    }

    fun typedLocal() {
        val client: WebClient = WebClient.create("https://typed-local.example.com")
        client.patch().uri("/reactive/typed").retrieve()
    }

    fun other(client: Other) {
        client.get().uri("/reactive/other")
    }
}

class Other {
    fun get() = this
    fun uri(path: String) = path
}
