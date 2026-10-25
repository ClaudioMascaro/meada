package com.example.api

import io.ktor.server.application.Application
import io.ktor.server.application.call
import io.ktor.server.response.respondText
import io.ktor.server.routing.get
import io.ktor.server.routing.post
import io.ktor.server.routing.route
import io.ktor.server.routing.routing

// The routes the web app reaches; their handlers call the clients below.
fun Application.module(
    prices: PriceClient,
    stock: StockClient,
    audit: AuditClient,
    promotions: PromotionClient,
    reports: ReportClient,
    inventory: InventoryClient,
    ledger: LedgerApi,
) {
    routing {
        route("/orders") {
            get("/{id}") {
                val id = call.parameters["id"] ?: ""
                prices.priceOf(id)
                stock.level()
                ledger.entry(id)
                call.respondText(id)
            }
            post("/reserve") {
                audit.record()
                promotions.current()
                reports.send()
                inventory.left(call.parameters["sku"] ?: "")
                call.respondText("reserved")
            }
        }
    }
}
