package com.example.api

import io.ktor.client.HttpClient
import io.ktor.client.engine.cio.CIO
import io.ktor.client.request.get
import org.springframework.beans.factory.annotation.Value
import org.springframework.stereotype.Component

// `@Value` on a constructor property Spring passes: `stock.url` is a literal URL.
@Component
class StockClient(@Value("\${stock.url}") private val base: String) {
    private val client = HttpClient(CIO)

    suspend fun level() {
        client.get("$base/api/stock")
    }
}
