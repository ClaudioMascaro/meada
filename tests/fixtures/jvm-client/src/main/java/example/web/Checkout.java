package example.web;

import example.models.Fixtures;
import example.models.Order;
import example.shared.Stamp;
import outside.absent.Thing;

public class Checkout {
    public int pay(int quantity) {
        Order order = Order.of(quantity);
        order.place(quantity, 1);
        order.place(quantity);
        return order.total();
    }

    public Order create() {
        return new Order(3);
    }

    public String stamp() {
        return Stamp.now();
    }

    public Order sample() {
        return Fixtures.sample();
    }

    public void external() {
        Thing.run();
    }
}
