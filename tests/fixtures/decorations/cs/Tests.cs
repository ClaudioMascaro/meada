namespace Example;

public class Tests
{
    [Fact]
    [InlineData(1)]
    public void Adds(int n)
    {
        Check(n);
    }

    private void Check(int n) { }
}
