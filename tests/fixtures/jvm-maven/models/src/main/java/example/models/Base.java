package example.models;

public class Base {
    public long find(long id) {
        return id;
    }

    public static Base of(int count) {
        return new Base();
    }

    public String name() {
        return "base";
    }

    public int size() {
        return 0;
    }
}
