using Api.Clients;
using Microsoft.AspNetCore.Mvc;

namespace Api.Controllers;

[ApiController]
[Route("orders")]
public class OrdersController : ControllerBase
{
    private readonly PriceClient _prices;
    private readonly StockClient _stock;
    private readonly AuditClient _audit;

    public OrdersController(PriceClient prices, StockClient stock, AuditClient audit)
    {
        _prices = prices;
        _stock = stock;
        _audit = audit;
    }

    // Reached by the web app through its API_URL variable.
    [HttpGet("{id}")]
    public async Task<string> Status(string id)
    {
        return await _prices.PriceOf(id);
    }

    // Reached by the web app through the api's declared host.
    [HttpPost("reserve")]
    public async Task Reserve()
    {
        await _stock.Level();
        await _audit.Record();
    }
}
