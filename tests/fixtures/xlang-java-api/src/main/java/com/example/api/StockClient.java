package com.example.api;

import org.springframework.beans.factory.annotation.Value;
import org.springframework.stereotype.Component;
import org.springframework.web.reactive.function.client.WebClient;

// `@Value` on a constructor parameter Spring passes: `stock.url` is a literal URL.
@Component
public class StockClient {
    private final WebClient web;

    public StockClient(@Value("${stock.url}") String base) {
        this.web = WebClient.create(base);
    }

    public void level() {
        web.get().uri("/api/stock").retrieve();
    }
}
