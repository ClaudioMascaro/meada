package example.declarative

import org.springframework.beans.factory.annotation.Value
import retrofit2.Retrofit

object Remotes {
    private const val HOST = "https://ledger.example.com"

    // The base is read from the builder chain that creates the interface, through the constant.
    fun ledger(): Ledger =
        Retrofit.Builder().baseUrl("$HOST/v1/").build().create(Ledger::class.java)
}

// A Retrofit held by a property creates the interface again, under another base.
object Shared {
    private val retrofit = Retrofit.Builder().baseUrl("https://shared.example.com/").build()

    fun ledger(): Ledger = retrofit.create(Ledger::class.java)
}

// Its callers reach the interface function like any call.
class Books(private val ledger: Ledger) {
    fun open(id: String) = ledger.entry(id)

    fun write() = ledger.record()
}

// The base from a `@Value` constructor property, and from the environment.
class Configured(@Value("https://billing.example.com") private val base: String) {
    fun ledger(): Ledger =
        Retrofit.Builder().baseUrl("$base/v2/").build().create(Ledger::class.java)

    fun fromEnvironment(): Ledger =
        Retrofit.Builder()
            .baseUrl(System.getenv("LEDGER_URL") ?: "https://fallback.example.com/")
            .build()
            .create(Ledger::class.java)
}
