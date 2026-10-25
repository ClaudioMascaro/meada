package com.example.api;

import org.springframework.cloud.openfeign.FeignClient;
import org.springframework.web.bind.annotation.GetMapping;
import org.springframework.web.bind.annotation.PathVariable;

// A declarative client: each method is a consumer, with the base `url` gives through the
// configuration (`inventory.url` is a literal URL the web app serves).
@FeignClient(name = "inventory", url = "${inventory.url}")
public interface InventoryClient {
    @GetMapping("/api/inventory/{sku}")
    String left(@PathVariable("sku") String sku);
}
