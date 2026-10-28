package ex;

class Box {
    void m(int a) {}
    void m(String a) {}
    void m(long a) {}

    void run(Object o) {
        m(o);
    }
}
