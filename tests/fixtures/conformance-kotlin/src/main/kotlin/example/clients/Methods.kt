package example.clients

import example.clients.urls.ORDERS
import io.ktor.client.request.*
import io.ktor.http.HttpMethod

// The method a wrapper takes is substituted at each call site, `this.method` or `method`.
suspend fun send(method: HttpMethod, path: String) =
    orders.request("$ORDERS$path") { this.method = method }

suspend fun sends() {
    send(HttpMethod.Delete, "/users/1")
    send(HttpMethod.Post, "/users")
}

suspend fun sendFrom(m: HttpMethod, path: String) = orders.request("$ORDERS$path") { method = m }

suspend fun sendsFrom() {
    sendFrom(HttpMethod.Put, "/m1")
}

// A constant URL: the call sites hold the method.
suspend fun sendFixed(m: HttpMethod) = billing.request("/m") { method = m }

suspend fun callsFixed() {
    sendFixed(HttpMethod.Patch)
}

// The block is the lambda argument wherever it is written.
suspend fun named() = orders.request(urlString = "$ORDERS/named") { method = HttpMethod.Post }

suspend fun blockNamed() =
    orders.request("$ORDERS/block-named", block = { method = HttpMethod.Put })

suspend fun thisPut() = orders.request("$ORDERS/this") { this.method = HttpMethod.Put }

// A method Meada cannot read is unknown, never `GET`: a block passed as a value, an assignment
// that may not run, one in a lambda inside the block, a method not named by a member.
suspend fun blockValue(block: HttpRequestBuilder.() -> Unit) =
    orders.request("$ORDERS/block", block)

suspend fun conditional(f: Boolean) =
    orders.request("$ORDERS/cond") { if (f) method = HttpMethod.Put }

suspend fun nested() = orders.request("$ORDERS/nested") { headers { method = HttpMethod.Post } }

suspend fun parsed() = orders.request("$ORDERS/parse") { method = HttpMethod.parse("POST") }
