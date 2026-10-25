package example.clients;

import org.springframework.http.HttpMethod;
import org.springframework.web.reactive.function.client.WebClient;

public class Reactive {
    static final WebClient ORDERS = WebClient.create("http://orders.example.com/api");
    static final WebClient BUILT =
            WebClient.builder().baseUrl("http://built.example.com").build();

    public void get() {
        ORDERS.get().uri("/orders/{id}", 1).retrieve();
    }

    public void flag() {
        BUILT.method(HttpMethod.PATCH).uri("/orders/1/flag").retrieve();
    }
}
