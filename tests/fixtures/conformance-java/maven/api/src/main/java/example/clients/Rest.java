package example.clients;

import example.web.Api;
import org.springframework.http.HttpMethod;
import org.springframework.web.client.RestTemplate;

public class Rest {
    private final RestTemplate rest;

    public Rest(RestTemplate rest) {
        this.rest = rest;
    }

    public String list() {
        return rest.getForObject(Api.ORDERS, String.class);
    }

    public String one(String id) {
        return rest.getForObject(Api.ORDERS + Api.ITEM, String.class, id);
    }

    public void create() {
        rest.postForObject(Api.ORDERS + "/new", null, String.class);
    }

    public void flag(int id) {
        rest.patchForObject(String.format(Api.ORDERS + "/%d/flag", id), null, String.class);
    }

    public void single() {
        rest.exchange(Api.ORDERS + "/single", HttpMethod.POST, null, String.class);
    }

    // A two-level wrapper: the method and the path come from its callers.
    void send(HttpMethod method, String path) {
        rest.exchange(Api.ORDERS + path, method, null, String.class);
    }

    void remove(String id) {
        send(HttpMethod.DELETE, "/" + id);
    }

    public void drop() {
        remove("7");
    }

    // Unresolved: the URL is no value Meada reads.
    public String lookup() {
        return rest.getForObject(System.getProperty("orders.url"), String.class);
    }
}
