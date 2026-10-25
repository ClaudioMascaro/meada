package example.clients.urls

const val ORDERS = "https://orders.example.com"

// A top-level property initialized with a template, never reassigned.
val USERS = "$ORDERS/users"

object Paths {
    const val ITEMS = "/items"
    val STOCK = ITEMS + "/stock"

    // Reassigned somewhere: Meada cannot say which value a request sees.
    var drifting = "/drifting"
}
