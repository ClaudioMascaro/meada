package example.clients

import io.ktor.client.HttpClient
import io.ktor.client.engine.cio.CIO
import io.ktor.client.features.defaultRequest
import io.ktor.client.request.*

// Ktor 1 spells `defaultRequest` in the package of its features.
val legacy = HttpClient(CIO) {
    defaultRequest { url("https://legacy.example.com/v1") }
}

suspend fun old() = legacy.get("/ping")
