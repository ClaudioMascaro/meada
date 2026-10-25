package example.jvm.fake

import example.clients.urls.ORDERS

// Types of the repository that take the names of the libraries': they are no client of theirs.
class RestTemplate {
    fun exchange(url: String) {}
}

class WebClient {
    fun get() = this

    fun uri(path: String) = path
}

fun probe(template: RestTemplate, client: WebClient) {
    template.exchange("$ORDERS/fake/rest")
    client.get().uri("$ORDERS/fake/web")
    val made = RestTemplate()
    made.exchange("$ORDERS/fake/made")
}
