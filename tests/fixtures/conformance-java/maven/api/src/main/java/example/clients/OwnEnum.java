package example.clients;

import org.springframework.web.client.RestTemplate;

// `HttpMethod` is this file's own enum, no Spring method: the call is unresolved.
public class OwnEnum {
    enum HttpMethod { GET }

    private final RestTemplate rest = new RestTemplate();

    public void get() {
        rest.exchange("/api/orders", HttpMethod.GET, null, String.class);
    }
}
