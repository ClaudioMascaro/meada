package example.web;

import example.models.*;

public class Probes {
    public int shadowed() {
        return Ledger.of(1);
    }

    public Object hidden(java.util.Optional<Integer> Order) {
        return Order.of(1);
    }

    public Object local() {
        java.util.Optional<Integer> Order = java.util.Optional.empty();
        return Order.of(1);
    }
}
