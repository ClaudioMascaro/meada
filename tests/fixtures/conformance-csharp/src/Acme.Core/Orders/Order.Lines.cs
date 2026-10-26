namespace Acme.Core.Orders;

public partial class Order
{
    private decimal Sum() => 0;

    public static Order Create() => new Order();
}
