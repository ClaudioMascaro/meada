package example;

import example.shared.Client;
import static example.shared.Helpers.*;

public class Store extends Base implements Comparable<Store> {
    private Client client;
    public static final String PREFIX = "store-";
    public Store() {}
    public String get(String key) { return Client.read(key); }
    protected String get(String key, int count) { return get(key); }
    private void hidden() { var copy = new Store(); copy.hidden(); }
    void reset() { client = null; }
    Runnable later() { return new Runnable() { public void run() { reset(); } }; }
}
// No field is read by reflection here, but the name getDeclaredField is spelled: `reflects`.
