namespace Acme.Core.Orders;

[Serializable]
public abstract partial class Carrier
{
    protected Carrier(string name) { }

    public virtual void Ship() { }

    [Obsolete("use Ship", DiagnosticId = "ACME1")]
    public void Track() { }

    protected void Rate() { }

    private void Purge() { }
}
