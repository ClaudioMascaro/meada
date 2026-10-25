package example.jvm

import java.net.URI
import java.net.http.HttpRequest
import java.net.http.HttpRequest.BodyPublishers

// The JDK's builder is the Java one, written without `new`.
class Jdk {
    fun list() = HttpRequest.newBuilder(URI.create("https://orders.example.com/jdk")).GET().build()

    fun create() = HttpRequest.newBuilder()
        .uri(URI.create("https://orders.example.com/jdk/new"))
        .POST(BodyPublishers.noBody())
        .build()
}
