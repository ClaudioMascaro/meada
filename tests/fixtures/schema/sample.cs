global using Example.Util;
using static Example.Util.Tools;

namespace Example;

public partial class Store
{
    public Store(string name) { }

    public int Save(int id, params string[] tags)
    {
        int Twice(int n) => n * 2;
        return Twice(Check(id));
    }

    public static Store Open() => new Store("store");
}
