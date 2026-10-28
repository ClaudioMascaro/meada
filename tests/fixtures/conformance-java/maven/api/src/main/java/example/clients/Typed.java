package example.clients;

import org.springframework.web.reactive.function.client.WebClient;

// A field of another class in the file does not hide the local; one of its own class does.
public class Typed {
    public void typed() {
        WebClient client = WebClient.create("https://example.org");
        client.get().uri("/java/typed").retrieve();
    }
}

class Shadowing {
    String client = "x";

    void hidden() {
        WebClient client = WebClient.create("https://example.org");
        client.get().uri("/java/hidden").retrieve();
    }
}
