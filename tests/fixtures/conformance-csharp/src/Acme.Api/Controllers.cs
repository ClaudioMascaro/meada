using Microsoft.AspNetCore.Mvc;

namespace Acme.Api;

// A class `[Route]` is the prefix: `[controller]` is the class name without `Controller`,
// `[action]` the method name, and `{id:int}`, `{id?}` are parameters.
[ApiController]
[Route("api/[controller]")]
public class OrdersController : ControllerBase
{
    [HttpGet]
    public void List() { }

    [HttpGet("{id:int}")]
    public void Get(int id) { }

    [HttpPost]
    public void Create() { }

    [HttpPut("{id}")]
    public void Replace(int id) { }

    [HttpPatch("{id}/[action]")]
    public void Touch(int id) { }

    [HttpDelete("{id?}")]
    public void Remove(int? id) { }

    [HttpHead]
    public void Probe() { }

    [HttpOptions]
    public void Allowed() { }

    // A template starting with `/` or `~/` replaces the prefix.
    [HttpGet("/health")]
    public void Health() { }

    [HttpGet("~/status/{code?}")]
    public void Status(int? code) { }

    // A `[Route]` without a method attribute serves every method.
    [Route("any/[action]")]
    public void Anything() { }

    // Not routable: no attribute.
    public void Helper() { }
}

// No `[ApiController]`: the base class makes a controller, and no prefix leaves the template.
public class PingController : Controller
{
    [HttpGet("ping")]
    public void Pong() { }
}

// A controller of a repository base class inherits its attributes: not read.
public class UsersController : ApiBase
{
    [HttpGet("users")]
    public void List() { }
}

[Route("v1")]
public class ApiBase : ControllerBase { }
