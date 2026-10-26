package com.example.api

import io.ktor.client.HttpClient
import io.ktor.client.engine.cio.CIO
import io.ktor.client.request.get
import org.springframework.beans.factory.annotation.Value
import org.springframework.stereotype.Component

// `@Value` on a property: `web.url` is `${WEB_URL}`, a base URL variable of the peer. The `$`
// is escaped in a Kotlin string.
@Component
class PriceClient {
    @Value("\${web.url}")
    private lateinit var webUrl: String
    private val client = HttpClient(CIO)

    suspend fun priceOf(sku: String) {
        client.get("$webUrl/api/prices/$sku")
    }
}
