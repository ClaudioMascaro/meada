namespace Api.Clients;

// A base from configuration: `Prices:BaseUrl` is a literal in appsettings.json whose host no peer
// serves, so the key maps to the variable `Prices__BaseUrl`.
public class PriceClient
{
    private readonly HttpClient _http = new HttpClient();

    public PriceClient(IConfiguration config)
    {
        _http.BaseAddress = new Uri(config["Prices:BaseUrl"]!);
    }

    public Task<string> PriceOf(string sku) => _http.GetStringAsync($"/api/prices/{sku}");
}
