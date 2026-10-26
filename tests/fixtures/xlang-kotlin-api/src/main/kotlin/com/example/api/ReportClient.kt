package com.example.api

import io.ktor.client.HttpClient
import io.ktor.client.engine.cio.CIO
import io.ktor.client.request.get

// An environment read: `WEB_URL` maps through base_url_env, whatever the fallback and the
// trailing slash the code trims.
class ReportClient {
    private val client = HttpClient(CIO)
    private val base = (System.getenv("WEB_URL") ?: "http://localhost:3000").removeSuffix("/")

    suspend fun send() {
        client.get("$base/api/reports")
    }
}
