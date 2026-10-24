package example;

import example.util.Tools;
import example.stars.*;
import static example.util.Tools.single;
import static example.util.Tools.*;

public class Orders {
    public Orders() {}
    public static void main(String[] args) { new Orders().start(); }
    public Orders(String id) { check(); }
    public static void place(String id, int count) {}
    public static void place(String id) {}
    public static void place(int count) {}
    public void start() {
        check();
        this.check();
        Orders.place("order", 2);
        Orders.place("order");
    }
    public void imported() {
        Tools.single();
        single();
        doubled();
        Wave.wave();
        Local.same();
        new Orders();
        new Orders("order");
        new Receipt("order");
        String.valueOf(1);
        System.currentTimeMillis();
    }
    public void enclosed() {
        Runnable action = () -> check();
        Runnable other = new Runnable() {
            public void run() { check(); }
        };
    }
    public void qualified(java.lang.String id) { this.check(); }
    public void shadow(Object Tools) { Tools.single(); }
    public void localShadow() {
        Object Tools = null;
        Tools.single();
    }
    protected void check() {}
    private void hidden() {}
    public static class Nested {
        public void run() {}
    }
    protected static class Protected {
        public void run() {}
    }
    private static class Private {
        public void run() {}
    }
    static class Package {
        public void run() {}
    }
}
