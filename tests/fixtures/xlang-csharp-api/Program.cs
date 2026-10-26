var builder = WebApplication.CreateBuilder(args);
builder.Services.AddControllers();
builder.Services.AddSingleton<Api.Clients.PriceClient>();
builder.Services.AddSingleton<Api.Clients.StockClient>();
builder.Services.AddSingleton<Api.Clients.AuditClient>();

// `builder.Configuration[...]` names a key too; `Promo:Url` is in no file, so it maps to Promo__Url.
var promotions = new HttpClient { BaseAddress = new Uri(builder.Configuration["Promo:Url"]!) };
await promotions.GetStringAsync("/api/promotions");

var app = builder.Build();
app.MapControllers();
app.Run();
