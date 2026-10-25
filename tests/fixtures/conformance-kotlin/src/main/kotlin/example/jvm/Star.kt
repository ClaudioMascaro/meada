package example.jvm

import example.clients.urls.ORDERS
import org.springframework.web.client.*
import org.springframework.web.reactive.function.client.*

// The clients come in by on-demand imports.
class Star {
    private val rest = RestTemplate()
    private val web = WebClient.create("https://star.example.com")

    fun rest() {
        rest.postForObject("$ORDERS/star/rest", null, String::class.java)
    }

    fun web() {
        web.get().uri("/star/web").retrieve()
    }
}
