package ex;

public class Box {
    void m(int a) {}
    void m(String a) {}
    void m(long a) {}

    public void run(Object o) {
        m(o);
    }
}
