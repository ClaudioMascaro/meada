namespace Acme.Core.Text
{
    public static class Format
    {
        public static string Upper(string value) => value;

        public static string Pad(string value) => value;

        public static string Pad(string value, int width) => value;

        internal static string Hidden() => "";
    }

    public static class Casing
    {
        public static string Lower(string value) => value;
    }

    public record Money(decimal Amount, string Currency);

    public struct Point
    {
        public int X;

        public Point(int x) { X = x; }

        public int Twice() => Double(X);

        private static int Double(int x) => x * 2;
    }

    public interface IClock
    {
        long Now();
    }
}
