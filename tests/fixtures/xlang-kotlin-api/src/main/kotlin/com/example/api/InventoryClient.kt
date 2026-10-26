package com.example.api

import io.ktor.client.HttpClient
import io.ktor.client.engine.cio.CIO
import io.ktor.client.request.get

// The map form of the environment read, with `trimEnd` and a fallback.
class InventoryClient {
    private val client = HttpClient(CIO)
    private val base = System.getenv()["WEB_URL"]?.trimEnd('/') ?: "http://localhost:3000"

    suspend fun left(sku: String) {
        client.get("$base/api/inventory/$sku")
    }
}
