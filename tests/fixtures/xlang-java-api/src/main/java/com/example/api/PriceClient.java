package com.example.api;

import org.springframework.beans.factory.annotation.Value;
import org.springframework.stereotype.Component;
import org.springframework.web.client.RestTemplate;

// `@Value` on a field: `web.url` is `${WEB_URL}`, a base URL variable of the peer.
@Component
public class PriceClient {
    @Value("${web.url}")
    private String webUrl;
    private final RestTemplate rest = new RestTemplate();

    public String priceOf(String sku) {
        return rest.getForObject(webUrl + "/api/prices/" + sku, String.class);
    }
}
