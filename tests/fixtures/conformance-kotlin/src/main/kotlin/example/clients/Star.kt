package example.clients

import io.ktor.client.*
import io.ktor.client.engine.cio.CIO
import io.ktor.client.request.*

// `HttpClient` and the request functions come in by on-demand imports, as Ktor's own examples
// write them.
val starred = HttpClient(CIO)

suspend fun star() = starred.get("https://star.example.com/one")
