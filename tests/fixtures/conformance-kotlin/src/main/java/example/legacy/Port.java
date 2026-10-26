package example.legacy;

// A Java class a Kotlin file extends: Java calls never reach Kotlin's extensions.
public class Port {
    public void ext(int x) {}

    void use(Port port, String s) {
        port.ext(1);
        s.length();
    }
}
