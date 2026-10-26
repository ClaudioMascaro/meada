package example.declarative

import retrofit2.Call
import retrofit2.http.*

// A Retrofit interface, its annotations imported on demand: each function is a consumer with the
// base of the builder that creates it (`Remotes`).
interface Ledger {
    @GET("entries/{id}")
    fun entry(@Path("id") id: String): Call<String>

    @POST("entries")
    fun record(): Call<String>
}
