package example.clients;

import org.springframework.web.client.RestTemplate;
import org.springframework.web.reactive.function.client.WebClient;

// A local declared once with an initializer holds its value: the environment read is the base.
public class Bases {
    private final RestTemplate rest;

    public Bases(RestTemplate rest) {
        this.rest = rest;
    }

    public void local() {
        String base = System.getenv("WEB_URL");
        rest.getForObject(base + "/api/local", String.class);
    }

    public void client() {
        WebClient client = WebClient.create("https://example.org");
        client.get().uri("/api/client").retrieve();
    }
}
