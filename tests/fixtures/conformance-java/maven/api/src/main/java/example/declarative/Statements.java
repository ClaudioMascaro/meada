package example.declarative;

// Callers reach the interface methods like any call: `trace` goes on to the provider.
public class Statements {
    private final OrdersClient orders;
    private final BillingClient billing;
    private final Ledger ledger = Remotes.ledger();

    public Statements(OrdersClient orders, BillingClient billing) {
        this.orders = orders;
        this.billing = billing;
    }

    public String report(String id) {
        ledger.entry(id);
        billing.invoice(id);
        return orders.find(id);
    }
}
