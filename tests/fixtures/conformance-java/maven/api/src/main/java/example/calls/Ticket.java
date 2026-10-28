package example.calls;

import java.io.Serializable;

// Known outside members: `Serializable` declares nothing and `Comparable` declares `compareTo`
// alone, so the class's own overloads are exact, `compareTo` stays a name match and a name
// nobody declares resolves to nothing.
public class Ticket implements Serializable, Comparable<Ticket> {
    public void open() {}
    public void open(int seat) {}
    public int compareTo(Ticket other) { return 0; }
    public void use(Ticket t) {
        t.open(1);
        open();
        t.compareTo(this);
        t.missing();
    }
}
