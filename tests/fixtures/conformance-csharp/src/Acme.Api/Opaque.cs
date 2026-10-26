using static Acme.Core.Text.Format;

namespace Acme.Api;

// A base from outside the repository may declare `Upper`: the call resolves to nothing.
public class Home : ControllerBase
{
    public void Index() { Upper("x"); }
}

// So does a base another part of the type writes.
public partial class Board : ControllerBase
{
}
