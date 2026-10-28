package example.jvm

import org.springframework.web.reactive.function.client.WebClient

fun make(): WebClient = WebClient.create("https://typed.example.com")

// A property of another class in the file does not hide the local; one of its own class does.
class Typed {
    fun typed() {
        val client: WebClient = make()
        client.get().uri("/persons/typed").retrieve()
    }
}

class Shadowing {
    private val client = "x"

    fun hidden() {
        val client: WebClient = make()
        client.get().uri("/persons/hidden").retrieve()
    }
}
