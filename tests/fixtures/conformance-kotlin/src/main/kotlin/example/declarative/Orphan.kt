package example.declarative

import retrofit2.Call
import retrofit2.http.GET

// No `create` names it: its base is unknown.
interface Orphan {
    @GET("/orphan")
    fun get(): Call<String>
}
