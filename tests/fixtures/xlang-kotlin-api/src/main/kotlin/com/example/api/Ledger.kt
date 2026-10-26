package com.example.api

import retrofit2.Retrofit
import retrofit2.http.*

// A Retrofit interface, its annotations imported on demand: each function is a consumer with the
// base of the builder that creates it, and the handlers call it like any function.
interface LedgerApi {
    @GET("api/ledger/{id}")
    suspend fun entry(@Path("id") id: String): String
}

object Ledgers {
    fun api(): LedgerApi =
        Retrofit.Builder().baseUrl("https://web.example.com/").build().create(LedgerApi::class.java)
}
