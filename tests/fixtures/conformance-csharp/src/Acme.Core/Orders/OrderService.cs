namespace Acme.Core.Orders;

public class OrderService
{
    public OrderService() : this("default") { }

    public OrderService(string region) { }

    public void Place(string sku, int count)
    {
        int Check(int n) => n;
        Check(count);
        Validate(sku);
        this.Audit();
        Place(sku);
        Notify(sku);
        var log = (string message) => Audit();
        Lower(sku);
        Fmt.Upper(sku);
        Format.Pad(sku, 2);
        Format.Pad(sku);
        new Order();
        Order.Create();
    }

    public void Place(string sku) { }

    protected void Audit() { }

    private void Validate(string sku) { }

    public void Notify(string message) { }

    public void Notify(int code) { }

    public static OrderService Default() => new OrderService();

    public class Line
    {
        public void Add() { Count(); }

        public int Count() => 0;
    }
}
