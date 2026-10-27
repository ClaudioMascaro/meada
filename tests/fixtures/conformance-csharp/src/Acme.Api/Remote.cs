using System.Threading.Tasks;
using Microsoft.Extensions.DependencyInjection;
using Refit;

namespace Acme.Api.Remote
{
    // A Refit interface: each method is a consumer with the base `RestService.For` makes it with.
    public interface IInventoryApi
    {
        [Get("/items/{sku}")]
        Task<string> Item(string sku);

        [Post("/items")]
        Task Add(string item);

        [Delete("/items/{sku}")]
        Task Delete(string sku);
    }

    // `AddRefitClient` is not read: the interface's base is unknown.
    public interface IReportsApi
    {
        [Get("/reports")]
        Task<string> All();
    }

    // `For<IOrdersApi>` gives its base to the endpoints it inherits from `IHealthApi` too.
    public interface IHealthApi
    {
        [Get("/ping")]
        Task Ping();
    }

    public interface IOrdersApi : IHealthApi
    {
        [Get("/orders")]
        Task<string> List();
    }

    // A caller of an interface method reaches it like any call.
    public class Inventory
    {
        private readonly IInventoryApi _api = RestService.For<IInventoryApi>("https://inventory.example.com/api");

        private readonly IOrdersApi _orders = RestService.For<IOrdersApi>("https://orders.example.com");

        public Task<string> Find(string sku) => _api.Item(sku);

        public static IServiceCollection AddReports(IServiceCollection services)
        {
            services.AddRefitClient<IReportsApi>();
            return services;
        }
    }
}
