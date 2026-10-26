namespace Acme.Api;

// A static `Main` starts a program; an instance one, or another name, does not.
public class Cli
{
    public static int Main(string[] args)
    {
        return args.Length;
    }

    public void Main(int code) { }

    static void Start() { }
}
