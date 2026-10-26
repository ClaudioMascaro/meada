// Top-level statements are a `Main` that starts a program, lambdas and local functions included;
// `Order` is bound by the project's `<Using>` alone.
var twice = (string s) => Shout(s);
Shout("x");
Order.Create();

void Shout(string s) { }
