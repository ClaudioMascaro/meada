namespace Example.Kit.Pricing;

public class Order
{
    private readonly int quantity;

    public Order(int quantity)
    {
        this.quantity = quantity;
    }

    public static Order Of(int quantity) => new Order(quantity);

    public int Total() => Tax(Subtotal());

    private int Subtotal() => quantity * 100;

    private int Tax(int amount) => amount + amount / 10;
}

internal class Audit
{
    public static void Run() { }
}
