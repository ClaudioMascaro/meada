package com.example.api;

import java.net.URI;
import java.net.http.HttpRequest;
import org.springframework.stereotype.Component;

// An environment read: `WEB_URL` maps through base_url_env.
@Component
public class ReportClient {
    public void send() {
        HttpRequest.newBuilder().uri(URI.create(System.getenv("WEB_URL") + "/api/reports")).build();
    }
}
