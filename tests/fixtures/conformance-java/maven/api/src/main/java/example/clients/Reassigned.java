package example.clients;

import org.springframework.web.client.RestTemplate;

// A local the file assigns holds no one value, so it is read as its written type alone.
public class Reassigned {
    private final RestTemplate rest;

    public Reassigned(RestTemplate rest) {
        this.rest = rest;
    }

    public void send(String other) {
        String base = System.getenv("WEB_URL");
        base = other;
        rest.getForObject(base + "/api/reassigned", String.class);
    }
}
