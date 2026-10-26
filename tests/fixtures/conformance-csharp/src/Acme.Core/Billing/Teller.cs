namespace Acme.Core.Billing;

public class Teller
{
    private readonly Account vault = new();
    private readonly IAuditor auditor;

    public Savings Primary { get; } = new Savings();

    public Teller(IAuditor auditor) { this.auditor = auditor; }

    public static Savings Open() => new Savings();

    public Account Current() => new Account();

    public void Run(Account given, IAuditor reviewer, List<Account> queue)
    {
        // A field, a property and a parameter, overloads narrowed by argument count.
        vault.Freeze();
        Primary.Accrue();
        given.Deposit(1);
        given.Deposit(1, "memo");
        given.Close(1);
        // A local with its type written, `var` initialized with `new`, a target-typed `new()`.
        Account explicitly = new Account();
        explicitly.Thaw();
        var made = new Savings();
        made.Accrue();
        made.Freeze();
        Account targeted = new();
        targeted.Thaw();
        // A method whose declared return type is written.
        var opened = Open();
        opened.Accrue();
        Open().Sweep();
        Current().Freeze();
        // An interface receiver reaches the interface member, a conditional access too.
        reviewer.Review();
        auditor?.Review();
        // An extension method, on a type of the repository and on one outside it.
        given.Summarize();
        queue.Archive();
        // A method the known type does not declare: a name match.
        given.Accrue();
        // A receiver of unknown type: a name match, every candidate when several declare it.
        var unknown = Mystery();
        unknown.Sweep();
        unknown.Review();
        unknown.Missing();
    }
}
