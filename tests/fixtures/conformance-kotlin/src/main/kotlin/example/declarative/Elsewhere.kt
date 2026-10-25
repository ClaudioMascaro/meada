package example.declarative

import retrofit2.Retrofit

// No `import retrofit2.create`: this `create` is the repository's, and creates nothing.
inline fun <reified T> Retrofit.create(): T = throw IllegalStateException()

object Elsewhere {
    private val retrofit = Retrofit.Builder().baseUrl("https://notes.example.com/").build()

    fun notes(): Notes = retrofit.create<Notes>()
}
