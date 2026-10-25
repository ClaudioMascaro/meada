package com.example.api;

import org.springframework.beans.factory.annotation.Value;
import org.springframework.stereotype.Component;
import org.springframework.web.client.RestTemplate;

// A placeholder with a default no peer serves: `PROMO_URL` maps through base_url_env.
@Component
public class PromotionClient {
    @Value("${PROMO_URL:http://promotions.internal/v2}")
    private String promotionsUrl;
    private final RestTemplate rest = new RestTemplate();

    public void current() {
        rest.getForObject(promotionsUrl + "/api/promotions", String.class);
    }
}
