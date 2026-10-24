package example.calls;

public interface Port {
    void send(String body);
    void send(String body, int retries);
}
