package example.calls;

public class Repo extends Store implements Port {
    public void save() {}
    public void touch(int id) {}
    public void mark(String key) {}
    public void send(String body) {}
    public void send(String body, int retries) {}
    public void run() {
        save(1);
        this.save(1);
        this.save();
        touch(1);
        mark(1);
        java.util.function.Consumer<Object> each = item -> item.ping();
    }
}
