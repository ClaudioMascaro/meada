namespace Acme.Core.Billing;

// A base from outside the repository may declare any member.
public class Pane : Window
{
    public void Touch()
    {
        Freeze();
        this.Thaw();
    }
}

public class Probes
{
    private Account slot = new();

    // The type is read through a generic, a nullable and a qualified name.
    public void Written(Ledger<Account> ledger, Account? maybe, global::Acme.Core.Billing.Account full, Pane pane)
    {
        ledger.Post(maybe);
        maybe.Freeze();
        full.Thaw();
        pane.Freeze();
    }

    // Two blocks bind `item` to two types, a method's type parameter names none: unknown.
    public void Blocks(bool flag)
    {
        if (flag) { Account item = new(); item.Freeze(); }
        else { Savings item = new(); item.Thaw(); }
    }

    public void Generic<T>(T value) { value.Freeze(); }

    // Every binding of a name hides the field: it is no longer the field's type.
    public void Hides(List<Savings> list, object any)
    {
        Savings slot = new();
        slot.Freeze();
        list.ForEach(slot => slot.Freeze());
        if (any is Savings slot) { slot.Freeze(); }
        slot.Freeze();
    }

    public void Field() { slot.Freeze(); }

    // A type outside the repository, or the repository's without the method, is no receiver of
    // any method of that name; an extension method may be.
    public void Outside(List<Account> items, Account given, IReviewer reviewer, IServices services)
    {
        items.Count();
        Teller.Accrue();
        given.Audit();
        reviewer.Review();
        reviewer.Escalate();
        services.AddOrders();
        services.AddOrders(config => { });
        ServiceExtensions.AddOrders(services);
        ServiceExtensions.AddOrders(services, null);
    }
}

// A `params` array of a primary constructor hides the type of its name.
public class Batch(params Format[] Format)
{
    public void Run() { Format.Upper("x"); }
}
