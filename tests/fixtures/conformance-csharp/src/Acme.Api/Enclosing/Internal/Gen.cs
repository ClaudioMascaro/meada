// No using: the namespaces around `Acme.Query.Internal` see their types, the innermost first.
namespace Acme.Query.Internal;

public class Gen
{
    public void M()
    {
        Util.Go();
        new Util();
    }
}

public class Sub : Util
{
    public void N() { Hello(); }
}
