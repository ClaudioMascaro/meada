package example.declarative

import retrofit2.Retrofit
import retrofit2.create

// Kotlin's extension takes the interface as a type argument.
object Reified {
    private val retrofit = Retrofit.Builder().baseUrl("https://archive.example.com/").build()

    fun archive(): Archive = retrofit.create<Archive>()

    // A Retrofit held by a local has the base of the chain that made it.
    fun journal(): Journal {
        val local = Retrofit.Builder().baseUrl("https://journal.example.com/").build()
        return local.create(Journal::class.java)
    }
}
