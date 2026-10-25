package client;

import org.springframework.http.HttpMethod;
import org.springframework.web.client.RestTemplate;

public class Orders {
    private final RestTemplate rest;

    Orders(RestTemplate rest) {
        this.rest = rest;
    }

    // A wrapper: its callers give the method and the path.
    void send(HttpMethod method, String path) {
        rest.exchange("/api/orders" + path, method, null, String.class);
    }

    // inferred: `@PutMapping("/{id}")` after the class prefix, through the wrapper.
    public void replace(String id) {
        send(HttpMethod.PUT, "/" + id);
    }

    // inferred, any method: `@RequestMapping("/any")` serves DELETE too.
    public void drop() {
        rest.delete("/api/orders/any");
    }

}
