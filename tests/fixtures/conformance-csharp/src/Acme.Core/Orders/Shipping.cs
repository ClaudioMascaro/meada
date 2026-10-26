namespace Acme.Core.Orders;

[Serializable]
public abstract class Carrier
{
    protected Carrier(string name) { }

    public virtual void Ship() { }

    [Obsolete("use Ship", DiagnosticId = "ACME1")]
    public void Track() { }

    protected void Rate() { }

    private void Purge() { }
}

public sealed class Courier : Carrier
{
    public Courier() : base("courier") { }

    public override void Ship()
    {
        base.Ship();
        Track();
    }
}
