// A minimal API (spec #28, #110): routes on the application and on nested groups, an extension
// method mapping routes on the group it is called on, a group used under two prefixes, a group
// passed to a method and one a method returns, a group mounted inside itself, and both handler
// forms. The SDK's implicit usings bind the framework's names.
using Acme.Web.Routes;

var builder = WebApplication.CreateBuilder(args);
var app = builder.Build();

app.MapGet("/health", () => "ok");
app.MapPost("/echo", Handlers.Echo);
app.MapMethods("/sync", new[] { "PUT", HttpMethods.Patch }, Handlers.Sync);

var api = app.MapGroup("/api");
var v1 = api.MapGroup("/v1");
v1.MapGroup("/orders").MapOrders();
api.MapGroup("/v2").MapGroup("/orders").MapOrders();
Admin.Map(api.MapGroup("/admin"));
var reports = Reports.Group(app);
reports.MapGet("/daily", Handlers.Daily);
app.MapGroup("/loop").Nest();

// A route builder Meada does not read (a lambda's parameter) serves nothing.
app.UseEndpoints(endpoints => endpoints.MapGet("/legacy", () => "old"));

// A lambda's parameter hides a local of its name only inside the lambda.
var shelf = new Shelf();
shelf.Stock();
var restock = (Crate shelf) => shelf.Stock();
