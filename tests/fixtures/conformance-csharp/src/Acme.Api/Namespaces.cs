using Acme.Core.Outer;

namespace Acme.Api
{
    public class Outside
    {
        // `using Acme.Core.Outer;` does not bind a type of `Acme.Core.Outer.Inner`.
        public void Run() { Second.Make(); }
    }
}
