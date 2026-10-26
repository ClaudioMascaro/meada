using Acme.Core.Text;

namespace Acme.Api;

// Each binding hides the type `Format` in its scope.
public class Shadows
{
    public void Parameter(object Format) { Format.Upper("a"); }

    public void Local() { var Format = new object(); Format.Upper("b"); }

    public void Lambda() { System.Action<object> f = Format => Format.Upper("c"); }

    public void Loop(object[] items) { foreach (var Format in items) { Format.Upper("d"); } }

    public void Caught() { try { } catch (System.Exception Format) { Format.Upper("e"); } }

    public void Out() { Parse(out var Format); Format.Upper("f"); }

    public void Pattern(object o) { if (o is string Format) { Format.Upper("g"); } }

    private void Parse(out object value) { value = null; }
}

public class FieldShadow
{
    private object Format;

    public void Run() { Format.Upper("h"); }
}

// A `readonly` field the constructor assigns from a parameter hides the type too.
public class ReadonlyShadow
{
    private readonly object Format;

    public ReadonlyShadow(object Format) { this.Format = Format; }

    public void Run() { Format.Upper("i"); }
}
