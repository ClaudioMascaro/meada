namespace Example.Money;

public static class Currency
{
    public static string Cents(int amount) => (amount / 100) + "." + (amount % 100);
}
