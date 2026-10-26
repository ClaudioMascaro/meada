using Example.Kit.Pricing;

namespace Example.App.Web;

public class Checkout
{
    public int Pay(int quantity)
    {
        Order order = Order.Of(quantity);
        return order.Total();
    }

    public Order Create() => new Order(3);
}
