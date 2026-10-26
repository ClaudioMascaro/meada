using System;
using System.Net.Http;
using System.Threading.Tasks;
using Microsoft.Extensions.DependencyInjection;

namespace Acme.Api.Typed
{
    // A typed client: its registration gives a base to the HttpClient the container passes to its
    // constructor, and every call on it inside the class takes that base.
    public class CatalogClient
    {
        private readonly HttpClient _http;

        public CatalogClient(HttpClient http) { _http = http; }

        public Task<string> Products() => _http.GetStringAsync("products");

        public Task<string> Product(int id) => this._http.GetStringAsync($"/products/{id}");
    }

    // A typed client that sets the base on the client its constructor takes.
    public class StockClient
    {
        private readonly HttpClient _http;

        public StockClient(HttpClient http)
        {
            http.BaseAddress = new Uri("https://stock.example.com/");
            _http = http;
        }

        public Task<string> Level() => _http.GetStringAsync("level");
    }

    // Registered with two bases: the base is unknown, never one of them.
    public class TwiceClient
    {
        private readonly HttpClient _http;

        public TwiceClient(HttpClient http) { _http = http; }

        public Task<string> Run() => _http.GetStringAsync("/twice");
    }

    // A typed client the code also builds itself (a target-typed `new`): the client is the
    // caller's, so the registration's base is not taken.
    public class BuiltClient
    {
        private readonly HttpClient _http;

        public BuiltClient(HttpClient http) { _http = http; }

        public Task<string> Run() => _http.GetStringAsync("/built");
    }

    public static class Registrations
    {
        public static IServiceCollection AddClients(this IServiceCollection services)
        {
            services.AddHttpClient<CatalogClient>(c => c.BaseAddress = new Uri("https://catalog.example.com/v1/"));
            services.AddHttpClient<TwiceClient>(c => c.BaseAddress = new Uri("https://one.example.com"));
            services.AddHttpClient<TwiceClient>(c => c.BaseAddress = new Uri("https://two.example.com"));
            services.AddHttpClient<BuiltClient>(c => c.BaseAddress = new Uri("https://built.example.com"));
            return services;
        }

        public static BuiltClient Build()
        {
            BuiltClient built = new(new HttpClient());
            return built;
        }
    }
}
