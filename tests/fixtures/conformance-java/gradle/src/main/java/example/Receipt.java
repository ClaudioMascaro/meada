package example;

public record Receipt(String id) {
    public Receipt {}
}
interface Pending {
    void finish();
}
enum State {
    OPEN, CLOSED;
}
