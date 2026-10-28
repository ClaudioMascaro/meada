package example.clients;

import org.springframework.beans.factory.annotation.Value;
import org.springframework.web.client.RestTemplate;

// A field `@Value` injects that the class assigns holds no one value.
public class Assigned {
    private final RestTemplate rest;

    @Value("http://assigned.example.com")
    private String base;

    public Assigned(RestTemplate rest) {
        this.rest = rest;
    }

    public void rebase(String other) {
        this.base = other;
    }

    public void send() {
        rest.getForObject(base + "/d", String.class);
    }
}
