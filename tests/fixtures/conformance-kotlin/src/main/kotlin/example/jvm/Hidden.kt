package example.jvm

import org.springframework.web.client.RestTemplate
import org.springframework.web.reactive.function.client.WebClient

fun anything(): Any = Any()

// A binding of the same name in a function hides the property: what it holds is not the client.
class Hidden {
    private val rest = RestTemplate()
    private val web = WebClient.create("https://hidden.example.com")

    fun local() {
        val rest = anything()
        rest.delete("https://hidden.example.com/local")
    }

    fun parameter(rest: Any) {
        rest.delete("https://hidden.example.com/parameter")
    }

    fun lambda(items: List<Any>) {
        items.forEach { rest -> rest.delete("https://hidden.example.com/lambda") }
    }

    fun client() {
        val web = anything()
        web.get().uri("/hidden/web")
    }

    // A loop variable or a `catch` parameter beside a `val` of the name: no one value holds.
    fun loop(items: List<Any>) {
        val web = WebClient.create("https://hidden-loop.example.com")
        for (web in items) web.get().uri("/hidden/loop")
    }

    fun caught() {
        val rest = RestTemplate()
        try {
            rest.delete("https://hidden.example.com/own")
        } catch (rest: Exception) {
            rest.delete("https://hidden.example.com/catch")
        }
    }

    // The property is the client where nothing hides it.
    fun own() {
        rest.delete("https://hidden.example.com/rest")
        web.get().uri("/hidden/own")
    }
}
