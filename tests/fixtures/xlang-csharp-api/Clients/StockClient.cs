namespace Api.Clients;

// A base from the environment: the variable WEB_URL, through a fallback and TrimEnd.
public class StockClient
{
    private static readonly HttpClient Http = new HttpClient();

    public async Task<string> Level()
    {
        var web = (Environment.GetEnvironmentVariable("WEB_URL") ?? "http://localhost:3000").TrimEnd('/');
        return await Http.GetStringAsync(web + "/api/stock");
    }
}
