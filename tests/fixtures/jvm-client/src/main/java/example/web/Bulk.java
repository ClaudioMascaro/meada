package example.web;

import static example.models.Order.of;

import example.models.*;

public class Bulk {
    public int total(int quantity) {
        return Order.of(quantity).total();
    }

    public void prepared() {
        of(2);
    }
}
