package example.jvm

import okhttp3.Request
import okhttp3.RequestBody

// A constructor call starts the builder: `GET` when the chain names no method.
class Ok(private val body: RequestBody) {
    fun list() = Request.Builder().url("https://orders.example.com/ok/orders").build()

    fun remove() = Request.Builder().url("https://orders.example.com/ok/orders/1").delete().build()

    fun create() = Request.Builder()
        .url("https://orders.example.com/ok/orders")
        .post(body)
        .build()

    fun replace() = Request.Builder().method("PUT", body).url("https://orders.example.com/ok/replace").build()
}

// Another library's `Request.Builder`, not OkHttp's.
fun notOkHttp() = com.acme.Request.Builder().url("https://orders.example.com/ok/other").build()
