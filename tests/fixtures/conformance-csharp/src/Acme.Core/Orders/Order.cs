namespace Acme.Core.Orders;

public partial class Order
{
    public decimal Total() => Sum();
}
