package example.jvm

import example.clients.urls.ORDERS
import org.springframework.http.HttpMethod
import org.springframework.web.client.RestTemplate

// A RestTemplate the constructor takes: every method sends its own, `exchange` names it.
class Rest(private val rest: RestTemplate) {
    fun list(): String? = rest.getForObject("$ORDERS/rest", String::class.java)

    fun one(id: String) = rest.getForEntity("$ORDERS/rest/$id", String::class.java)

    fun create() {
        rest.postForObject("$ORDERS/rest/new", null, String::class.java)
        rest.postForEntity("$ORDERS/rest/created", null, String::class.java)
    }

    fun change(id: String) {
        rest.put("$ORDERS/rest/$id", null)
        rest.patchForObject("$ORDERS/rest/$id/flag", null, String::class.java)
    }

    fun single() {
        rest.exchange("$ORDERS/rest/single", HttpMethod.POST, null, String::class.java)
    }

    // A two-level wrapper: the method and the path come from its callers.
    fun send(method: HttpMethod, path: String) {
        rest.exchange("$ORDERS/rest$path", method, null, String::class.java)
    }

    fun remove(id: String) = send(HttpMethod.DELETE, "/$id")

    fun drop() = remove("7")

    // Unresolved: the URL is no value Meada reads.
    fun lookup(): String? = rest.getForObject(System.getProperty("orders.url"), String::class.java)
}

// A RestTemplate made by its constructor, held by a property and by a local.
class Made {
    private val rest = RestTemplate()

    fun property() {
        this.rest.delete("$ORDERS/rest/made")
    }

    fun local() {
        val template = RestTemplate()
        template.getForObject("$ORDERS/rest/local", String::class.java)
    }
}

// What is no RestTemplate sends nothing.
class Fake {
    fun delete(path: String) {}

    fun probe(fake: Fake, map: Map<String, String>) {
        fake.delete("$ORDERS/rest/fake")
        map.get("$ORDERS/rest/map")
    }
}
