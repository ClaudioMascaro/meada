// Two namespaces in one file: each type binds under its own.
namespace Acme.Core.Outer
{
    public class First { }

    namespace Inner
    {
        public class Second
        {
            public static void Make() { }
        }
    }
}
