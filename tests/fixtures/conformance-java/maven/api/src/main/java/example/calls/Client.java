package example.calls;

public class Client {
    private Repo repo;
    private Port port;
    private example.calls.Repo qualified;

    public Store make() { return new Repo(); }

    public void typed(Repo given) {
        repo.save();
        repo.save(1);
        repo.ping();
        this.repo.touch(1);
        given.touch(2);
        Repo local = given;
        local.mark(3);
        var fresh = new Repo();
        fresh.save();
        var made = make();
        made.save(1);
        qualified.save();
        port.send("body");
        port.send("body", 2);
    }

    public void chained() {
        make().save(1);
    }

    public void shadowed(Port repo) {
        repo.send("body");
        Store Repo = new Store();
        Repo.mark(1);
    }

    public void unknown() {
        java.util.function.Consumer<Object> each = item -> {
            item.flush();
            item.ping();
        };
    }
}
