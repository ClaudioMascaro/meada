// The nested block form of `Enclosing/Internal/Gen.cs`.
namespace Acme
{
    namespace Block
    {
        namespace Internal
        {
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
        }
    }
}
