using System;
using System.Net.Http;
using System.Net.Http.Json;
using System.Threading.Tasks;
using Microsoft.Extensions.Configuration;

namespace Acme.Api.Clients
{
    public static class Hosts
    {
        public const string Billing = "https://billing.example.com";
        public static readonly string Invoices = Billing + "/api/invoices";
    }

    // Every HttpClient form, with absolute URLs.
    public class Verbs
    {
        private readonly HttpClient http;

        public Verbs(HttpClient http) { this.http = http; }

        public async Task Run(int id)
        {
            await http.GetAsync("https://orders.example.com/orders");
            await http.PostAsync("https://orders.example.com/orders", null);
            await http.PutAsync("https://orders.example.com/orders/1", null);
            await http.PatchAsync("https://orders.example.com/orders/2", null);
            await http.DeleteAsync("https://orders.example.com/orders/3");
            await http.GetStringAsync("https://orders.example.com/text");
            await http.GetStreamAsync("https://orders.example.com/stream");
            await http.GetByteArrayAsync("https://orders.example.com/bytes");
            await http.GetFromJsonAsync<int[]>("https://orders.example.com/json");
            await http.PostAsJsonAsync("https://orders.example.com/json", id);
            await http.PutAsJsonAsync("https://orders.example.com/json/1", id);
            await http.SendAsync(new HttpRequestMessage(HttpMethod.Delete, "https://orders.example.com/send"));
            await http.SendAsync(new HttpRequestMessage(HttpMethod.Post, new Uri("https://orders.example.com/uri")));
        }
    }

    // A BaseAddress from the client's initializer, from an assignment and from a constant.
    public class Based
    {
        private static readonly HttpClient Shared = new HttpClient { BaseAddress = new Uri("https://shared.example.com/api/") };
        private readonly HttpClient field = new HttpClient { BaseAddress = new Uri(Hosts.Billing) };

        public async Task Run()
        {
            await Shared.GetAsync("items");
            await Shared.GetAsync("/root");
            await field.GetAsync("/billing");
            var local = new HttpClient();
            local.BaseAddress = new Uri("https://local.example.com/v1");
            await local.GetStringAsync("orders/1");
            // A relative URL replaces the last segment of a base without a trailing slash.
            var slash = new HttpClient();
            slash.BaseAddress = new Uri("https://slash.example.com/v1/");
            await slash.GetStringAsync("orders/2");
        }
    }

    // Strings and constants folded into a URL, including those of other types.
    public class Folds
    {
        private readonly HttpClient http;
        private const string Local = "https://local.example.com";

        public Folds(HttpClient http) { this.http = http; }

        public async Task Run(string id)
        {
            await http.GetAsync($"{Hosts.Billing}/invoices/{id}");
            await http.GetAsync(Hosts.Invoices);
            await http.GetAsync(Routes.Items + "/stock");
            await http.GetAsync(Local + "/plus/" + id);
            await http.GetAsync(@"https://verbatim.example.com/a");
            await http.GetAsync($@"https://verbatim.example.com/{id}/b");
            await http.GetAsync($"https://format.example.com/{id:D4}/{{braces}}");
        }
    }

    // A helper that wraps a client: each call site is a consumer.
    public class Wrappers
    {
        private readonly HttpClient http;

        public Wrappers(HttpClient http) { this.http = http; }

        private Task<string> Fetch(string path) => http.GetStringAsync($"https://wrapped.example.com{path}");

        private Task<string> ForOrder(string id) => Fetch("/orders/" + id);

        public async Task Run(string id)
        {
            await ForOrder("7");
            await ForOrder(id);
            await Fetch("/direct");
        }
    }

    // A path that cannot be evaluated, and a client made by the factory, which is not read.
    public class Unread
    {
        private readonly HttpClient http;
        private readonly IHttpClientFactory factory;

        public Unread(HttpClient http, IHttpClientFactory factory)
        {
            this.http = http;
            this.factory = factory;
        }

        private string Lookup() => "https://lookup.example.com/x";

        public async Task Run()
        {
            await http.GetAsync(Lookup());
            HttpClient named = factory.CreateClient("orders");
            await named.GetAsync("/named");
        }
    }

    // A name assigned again holds no one value: the base or the request is unknown, never the
    // first one written.
    public class Reassigned
    {
        private readonly HttpClient swapped = new HttpClient { BaseAddress = new Uri("https://default.example.com") };
        private readonly HttpClient twice = new HttpClient { BaseAddress = new Uri("https://twice.example.com") };
        private readonly HttpClient shadow;
        private readonly HttpClient http;

        public Reassigned(HttpClient client)
        {
            swapped = client;
            http = client;
            this.shadow.BaseAddress = new Uri("https://field.example.com");
        }

        public void Prod() { twice.BaseAddress = new Uri("https://prod.example.com"); }

        public void Stage() { twice.BaseAddress = new Uri("https://stage.example.com"); }

        public async Task Run(bool flag)
        {
            await swapped.GetAsync("/swapped");
            await twice.GetAsync("/twice");

            var c = new HttpClient { BaseAddress = new Uri("https://first.example.com") };
            c = new HttpClient { BaseAddress = new Uri("https://second.example.com") };
            await c.GetAsync("/client");

            var branch = new HttpClient { BaseAddress = new Uri("https://default.example.com") };
            if (flag) branch.BaseAddress = new Uri("https://prod.example.com");
            else branch.BaseAddress = new Uri("https://stage.example.com");
            await branch.GetAsync("/branch");

            var req = new HttpRequestMessage(HttpMethod.Get, "https://req.example.com/a");
            req = new HttpRequestMessage(HttpMethod.Post, "https://req.example.com/b");
            await http.SendAsync(req);

            string url = "https://again.example.com/a";
            url += "/more";
            await http.GetAsync(url);
        }

        // A parameter that hides the field keeps the base of its caller.
        public async Task Hidden(HttpClient shadow) { await shadow.GetAsync("/hidden"); }

        public async Task Use() { await Hidden(new HttpClient { BaseAddress = new Uri("https://arg.example.com") }); }

        // A nested type has a field of its own.
        public class Inner
        {
            private readonly HttpClient shadow;

            public async Task Run() { await shadow.GetAsync("/inner"); }
        }
    }

    // What the source reads about a client and its request.
    public class Reads
    {
        private readonly HttpClient http;
        private readonly System.Net.Http.HttpClient qualified;
        private readonly HttpClient pooled;
        public HttpClient Client { get; } = new HttpClient { BaseAddress = new Uri("https://property.example.com") };

        public Reads(HttpClient http, IHttpClientFactory factory)
        {
            this.http = http;
            pooled = factory.CreateClient("orders");
        }

        private string Config() => "https://config.example.com";

        public async Task Run(int id)
        {
            var user = $"https://locals.example.com/u/{id}";
            string text = "https://locals.example.com/s";
            const string fixedUrl = "https://locals.example.com/c";
            await http.GetAsync(user);
            await http.GetAsync(text);
            await http.GetAsync(fixedUrl);

            var put = new HttpRequestMessage(HttpMethod.Get, "https://method.example.com/m");
            put.Method = HttpMethod.Put;
            await http.SendAsync(put);

            await http?.GetAsync("https://null.example.com/n");
            await qualified.GetAsync("https://qualified.example.com/q");
            await pooled.GetAsync("/named-field");
            await Client.GetAsync("/property");

            var unknown = new HttpClient { BaseAddress = new Uri(Config() + "/api/") };
            await unknown.GetAsync("/rooted");
        }
    }

    // An environment read, and a key of the project's appsettings.json, in a URL.
    public class Settings
    {
        private readonly HttpClient http;
        private readonly IConfiguration config;

        public Settings(HttpClient http, IConfiguration config)
        {
            this.http = http;
            this.config = config;
        }

        public async Task Run()
        {
            await http.GetAsync(Environment.GetEnvironmentVariable("BILLING_URL") + "/api/invoices");
            await http.GetAsync(config["Services:Stock"] + "/levels");
        }
    }

    // A GetAsync of the repository does not make the client's call one of its own.
    public class Repo
    {
        public Task GetAsync(string path) => Task.CompletedTask;
    }
}
