package example.models;

public class Order {
    public Order(int quantity) {
    }

    public static Order of(int quantity) {
        return new Order(quantity);
    }

    public int total() {
        return tax(subtotal());
    }

    public int place(int quantity) {
        return quantity;
    }

    public int place(String name) {
        return name.length();
    }

    public int place(int quantity, int extra) {
        return quantity + extra;
    }

    int subtotal() {
        return 10;
    }

    private int tax(int amount) {
        return amount;
    }
}
