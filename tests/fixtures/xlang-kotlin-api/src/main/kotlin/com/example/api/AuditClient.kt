package com.example.api

import io.ktor.client.HttpClient
import io.ktor.client.engine.cio.CIO
import io.ktor.client.request.post
import org.springframework.beans.factory.annotation.Value
import org.springframework.stereotype.Component

// A placeholder with a default whose host a peer serves: the default's host links.
@Component
class AuditClient {
    @Value("\${audit.url}")
    private lateinit var auditUrl: String
    private val client = HttpClient(CIO)

    suspend fun record() {
        client.post(auditUrl + "/audit")
    }
}
