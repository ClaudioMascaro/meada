using Microsoft.AspNetCore.Mvc;

namespace Acme.Api.Edges;

public partial class PartsController : ControllerBase
{
    [HttpGet("part")]
    public void Part() { }
}
