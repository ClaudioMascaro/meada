package example.clients;

import java.net.URI;
import java.net.http.HttpRequest;

public class Jdk {
    static final String BASE = "http://orders.example.com/api";

    public HttpRequest list() {
        return HttpRequest.newBuilder().uri(URI.create(BASE + "/orders")).build();
    }

    public HttpRequest create() {
        return HttpRequest.newBuilder(URI.create(BASE + "/orders/new")).POST(null).build();
    }

    public HttpRequest replace() {
        return HttpRequest.newBuilder().method("PUT", null).uri(URI.create(BASE + "/orders/1")).build();
    }
}
