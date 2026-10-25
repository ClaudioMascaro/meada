package example.declarative

import retrofit2.Call
import retrofit2.http.*

// Created by the extension `create<Archive>()` (`Reified`).
interface Archive {
    @GET("archive/{id}")
    fun find(@Path("id") id: String): Call<String>
}

// Created by a Retrofit a local holds (`Reified`).
interface Journal {
    @DELETE("journal/{id}")
    fun drop(@Path("id") id: String): Call<String>
}

// Created by an extension of the repository named `create` (`Elsewhere`): no Retrofit's.
interface Notes {
    @PUT("notes")
    fun save(): Call<String>
}
