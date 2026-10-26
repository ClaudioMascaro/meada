namespace Api.Clients;

// A base from the environment held by a local: the variable WEB_URL, plain and interpolated.
public class SiteClient
{
    private static readonly HttpClient Http = new HttpClient();

    public async Task<string> Pages(string sku)
    {
        var web = Environment.GetEnvironmentVariable("WEB_URL");
        var concatenated = await Http.GetStringAsync(web + $"/api/prices/{sku}");
        return concatenated + await Http.GetStringAsync($"{web}/api/promotions");
    }
}
