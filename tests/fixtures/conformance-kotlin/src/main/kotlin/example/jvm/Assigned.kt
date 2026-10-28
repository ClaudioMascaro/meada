package example.jvm

import org.springframework.beans.factory.annotation.Value
import org.springframework.web.client.RestTemplate

// A property `@Value` injects that the class assigns holds no one value.
class Assigned(private val rest: RestTemplate) {
    @Value("https://assigned.example.com")
    private lateinit var base: String

    fun rebase(other: String) {
        this.base = other
    }

    fun send() {
        rest.getForObject("$base/d", String::class.java)
    }
}
