package example.calls;

import java.util.ArrayList;

public abstract class Listing extends ArrayList<String> implements Port {
    public void own() {}
    public void use() {
        own();
        this.own();
        this.send("body");
    }
    class Task implements Runnable {
        public void run() { own(); }
    }
}
