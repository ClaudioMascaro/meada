// A using brings another `Util`: the enclosing namespace's binds before it.
using Other;

namespace Acme.Query.Imports;

public class Used
{
    public void M() { Util.Go(); }
}
