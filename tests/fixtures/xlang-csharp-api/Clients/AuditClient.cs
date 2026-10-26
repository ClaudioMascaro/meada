namespace Api.Clients;

// A base from configuration whose literal host the web app serves: the call links by that host.
public class AuditClient
{
    private readonly IConfiguration _config;
    private readonly HttpClient _http = new HttpClient();

    public AuditClient(IConfiguration config)
    {
        _config = config;
    }

    public async Task Record()
    {
        await _http.PostAsync(_config["Audit:Url"] + "audit", null);
    }
}
