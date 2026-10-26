namespace Acme.Core.Billing;

public interface IAuditor
{
    void Review();
}

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

    public static void Archive(this IEnumerable<Account> accounts) { }
}

public class Ledger<T>
{
    // A type parameter names no type of the repository.
    public void Post(T entry) { entry.Freeze(); }
}
