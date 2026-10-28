using System;

namespace Acme.Api;

// Known outside members: `IDisposable` declares `Dispose` and `IComparable` `CompareTo` alone,
// so the class's own overloads are exact, `Dispose` stays a name match and a name nobody
// declares resolves to nothing.
public class Lease : IDisposable, IComparable
{
    public void Open() { }
    public void Open(int seat) { }
    public void Dispose() { }
    public int CompareTo(object other) { return 0; }

    public void Use(Lease l)
    {
        l.Open(1);
        Open();
        l.Dispose();
        l.Missing();
    }
}
