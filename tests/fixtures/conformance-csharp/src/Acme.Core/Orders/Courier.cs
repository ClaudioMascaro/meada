namespace Acme.Core.Orders;

public sealed class Courier : Carrier
{
    public Courier() : base("courier") { }

    public override void Ship()
    {
        base.Ship();
        Track();
        Bill();
    }
}
