package example.clients;

import org.springframework.beans.factory.annotation.Value;
import org.springframework.web.client.RestTemplate;

// The bases Spring's `@Value` injects: a literal, a placeholder the module's configuration
// holds, and a placeholder with a default.
public class Injected {
    private final RestTemplate rest;

    @Value("http://literal.example.com")
    private String literal;

    @Value("${billing.url}")
    private String configured;

    @Value("${BILLING_URL:http://fallback.example.com}")
    private String fallback;

    public Injected(RestTemplate rest) {
        this.rest = rest;
    }

    public void send() {
        rest.getForObject(literal + "/a", String.class);
        rest.getForObject(configured + "/b", String.class);
        rest.getForObject(fallback + "/c", String.class);
    }
}
