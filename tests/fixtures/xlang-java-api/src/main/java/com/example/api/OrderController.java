package com.example.api;

import org.springframework.web.bind.annotation.GetMapping;
import org.springframework.web.bind.annotation.PathVariable;
import org.springframework.web.bind.annotation.PostMapping;
import org.springframework.web.bind.annotation.RequestMapping;
import org.springframework.web.bind.annotation.RestController;

@RestController
@RequestMapping("/orders")
public class OrderController {
    private final PriceClient prices;
    private final StockClient stock;
    private final AuditClient audit;
    private final PromotionClient promotions;
    private final ReportClient reports;

    OrderController(PriceClient prices, StockClient stock, AuditClient audit,
            PromotionClient promotions, ReportClient reports) {
        this.prices = prices;
        this.stock = stock;
        this.audit = audit;
        this.promotions = promotions;
        this.reports = reports;
    }

    @GetMapping("/{id}")
    public String status(@PathVariable String id) {
        prices.priceOf(id);
        stock.level();
        return id;
    }

    @PostMapping("/reserve")
    public void reserve() {
        audit.record();
        promotions.current();
        reports.send();
    }
}
