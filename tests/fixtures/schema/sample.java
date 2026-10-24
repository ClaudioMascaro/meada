package example;

import example.shared.Client;
import static example.shared.Helpers.*;

public class Store {
    public static final String PREFIX = "store-";
    public Store() {}
    public String get(String key) { return Client.read(key); }
    protected String get(String key, int count) { return get(key); }
    private void hidden() {}
}
