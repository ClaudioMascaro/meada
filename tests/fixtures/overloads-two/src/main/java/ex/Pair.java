package ex;

class Left {
    void m(int a) {}
    void m(String a) {}
}

class Right {
    void m(int a) {}
    void m(String a) {}
}

class Pair {
    void run(Object o) {
        java.util.function.Consumer<Object> each = x -> {
            x.m(o);
        };
    }
}
