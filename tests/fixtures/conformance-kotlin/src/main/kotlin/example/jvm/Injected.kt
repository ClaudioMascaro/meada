package example.jvm

import org.springframework.beans.factory.annotation.Value
import org.springframework.web.client.RestTemplate

// The bases Spring's `@Value` injects, and the environment: a literal, a placeholder the
// module's configuration holds, and `System.getenv` with a fallback.
class Injected(private val rest: RestTemplate) {
    @Value("https://literal.example.com")
    private lateinit var literal: String

    @Value("\${billing.url}")
    private lateinit var configured: String

    private val fallback = System.getenv("BILLING_URL") ?: "https://fallback.example.com"

    fun send() {
        rest.getForObject("$literal/a", String::class.java)
        rest.getForObject("$configured/b", String::class.java)
        rest.getForObject("$fallback/c", String::class.java)
    }
}
