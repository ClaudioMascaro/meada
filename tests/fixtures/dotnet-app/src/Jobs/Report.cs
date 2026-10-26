using Example.Money;
using Example.Shared;
using static Example.Money.Currency;

namespace Example.App.Jobs;

public class Report
{
    public string Line(int amount) => Stamp.Now() + Currency.Cents(amount);

    public string Short(int amount) => Cents(amount);
}
