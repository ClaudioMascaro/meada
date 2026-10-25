package example.models;

public class Repo extends Base {
    public String find(String key) {
        return key;
    }

    public static Repo of(String key) {
        return new Repo();
    }

    public String name() {
        return "repo";
    }
}
