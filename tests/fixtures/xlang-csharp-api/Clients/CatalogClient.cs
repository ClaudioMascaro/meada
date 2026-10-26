namespace Api.Clients;

// A typed client: `AddHttpClient<CatalogClient>` in Program.cs gives the client the container
// passes to the constructor a base from configuration, `Catalog:Url`, whose literal host the web
// app serves.
public class CatalogClient
{
    private readonly HttpClient _http;

    public CatalogClient(HttpClient http)
    {
        _http = http;
    }

    public Task<string> Products() => _http.GetStringAsync("catalog");
}
