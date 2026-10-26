package com.example.api

import io.ktor.client.HttpClient
import io.ktor.client.engine.cio.CIO
import io.ktor.client.request.get
import org.springframework.beans.factory.annotation.Value
import org.springframework.stereotype.Component

// A placeholder with a default no peer serves: `PROMO_URL` maps through base_url_env.
@Component
class PromotionClient {
    @Value("\${PROMO_URL:http://promotions.internal/v2}")
    private lateinit var promotionsUrl: String
    private val client = HttpClient(CIO)

    suspend fun current() {
        client.get("$promotionsUrl/api/promotions")
    }
}
