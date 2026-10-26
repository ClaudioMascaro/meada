using Acme.Core.Billing;

namespace Acme.Api;

// A type of another project, through a field and a primary constructor's parameter.
public class Desk
{
    private readonly Teller teller;

    public Desk(Teller teller) { this.teller = teller; }

    public void Serve(Account account) { teller.Run(account, new Auditor(), null); }
}

public class Booth(Teller teller)
{
    public void Open() { teller.Current(); }
}
