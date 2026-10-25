package com.example.api;

import org.springframework.beans.factory.annotation.Value;
import org.springframework.stereotype.Component;
import org.springframework.web.client.RestTemplate;

// A placeholder with a default whose host a peer serves: the default's host links.
@Component
public class AuditClient {
    @Value("${audit.url}")
    private String auditUrl;
    private final RestTemplate rest = new RestTemplate();

    public void record() {
        rest.postForObject(auditUrl + "/audit", null, String.class);
    }
}
