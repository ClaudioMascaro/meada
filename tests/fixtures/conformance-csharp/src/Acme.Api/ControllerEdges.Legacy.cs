using Microsoft.AspNetCore.Mvc;

namespace Acme.Api.Legacy;

// A `Route` of the repository is no MVC attribute: the class has a prefix that is not read.
public class Route { }

[ApiController]
[Route("legacy")]
public class OldController : ControllerBase
{
    [HttpGet("old")]
    public void Old() { }
}
