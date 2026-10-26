namespace Acme.Web.Routes;

public static class Handlers
{
    public static string Echo(string body) => body;

    public static void Sync() { }

    public static string Daily() => "daily";

    public static string Find(int id) => Store.All();

    public static void Ban(int id) { }
}

static class Store
{
    public static string All() => "all";

    public static void Remove(int id) { }
}

public class Shelf
{
    public void Stock() { }
}

public class Crate
{
    public void Stock() { }
}
