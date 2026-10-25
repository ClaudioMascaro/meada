package com.example.api

import io.ktor.client.HttpClient
import io.ktor.client.engine.cio.CIO
import io.ktor.client.request.get
import org.springframework.beans.factory.annotation.Value
import org.springframework.stereotype.Component

// A property some code assigns holds no one value: what Spring injects is not its URL, so the
// consumer is unresolved.
@Component
class MirrorClient {
    @Value("\${stock.url}")
    private lateinit var mirrorUrl: String
    private val client = HttpClient(CIO)

    fun reset() {
        mirrorUrl = "http://localhost:9999"
    }

    suspend fun mirror() {
        client.get("$mirrorUrl/api/mirror")
    }
}
