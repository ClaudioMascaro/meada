namespace Acme.Core.Billing;

public interface IAuditor
{
    void Review();
}

public interface IReviewer : IAuditor
{
    void Escalate();
}

public interface IServices { }

public class Auditor : IAuditor
{
    public void Review() { }
}

public class Account
{
    public void Deposit(int amount) { }

    public void Deposit(int amount, string memo) { }

    public void Close(int code) { }

    public void Close(string reason) { }

    public void Freeze() { }

    public void Thaw() { }
}

public class Savings : Account
{
    public void Accrue() { }

    // Unqualified and `this.` calls look in the base the declaration writes.
    public void Sweep()
    {
        Freeze();
        this.Thaw();
    }
}

public static class AccountExtensions
{
    public static void Summarize(this Account account) { }

    public static void Audit(this Account account) { }

    public static void Archive(this IEnumerable<Account> accounts) { }
}

public class Ledger<T>
{
    // A type parameter names no type of the repository.
    public void Post(T entry) { entry.Freeze(); }
}

public static class SavingsExtensions
{
    public static void Audit(this Savings savings) { }
}

// Overloads of an extension method: a call on a receiver takes one argument fewer.
public static class ServiceExtensions
{
    public static void AddOrders(this IServices services) { }

    public static void AddOrders(this IServices services, Action<Account> configure) { }
}
