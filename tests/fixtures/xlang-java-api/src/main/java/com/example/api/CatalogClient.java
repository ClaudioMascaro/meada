package com.example.api;

import org.springframework.cloud.openfeign.FeignClient;
import org.springframework.web.bind.annotation.GetMapping;

// Without `url` the host is unresolved: one provider serving the path and method links it, and
// the client's name is in the reason.
@FeignClient("catalog")
public interface CatalogClient {
    @GetMapping("/api/catalog")
    String all();
}
