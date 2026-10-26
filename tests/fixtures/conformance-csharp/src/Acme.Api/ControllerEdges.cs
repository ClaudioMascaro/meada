using Microsoft.AspNetCore.Mvc;

// Routes this reader gets wrong are left out: each controller below serves what the comment says.
namespace Acme.Api.Edges
{
    // `[action]` drops `Async`, and a literal `[ActionName]` renames the action.
    [Route("edges/[controller]/[action]")]
    public class JobsController : ControllerBase
    {
        [HttpGet]
        public void ListAsync() { }

        [HttpGet, ActionName("done")]
        public void FinishAsync() { }

        [HttpGet, ActionName(Names.Skip)]
        public void Skipped() { }

        [NonAction, HttpGet("hidden")]
        public void Helper() { }

        [HttpGet("internal")]
        void Internal() { }
    }

    // No class `[Route]` and no template: routed by convention, which is not read.
    public class PagesController : Controller
    {
        [HttpGet]
        public void Index() { }
    }

    // `[Route("")]` is the root.
    [Route("")]
    public class RootController : Controller
    {
        [HttpGet]
        public void Index() { }
    }

    // A base class of the repository is not a controller of its own: its attributes are
    // inherited by `GadgetsController`, which is not read either.
    [ApiController]
    [Route("edges/[controller]")]
    public abstract class CrudController : ControllerBase
    {
        [HttpGet]
        public void All() { }
    }

    public class GadgetsController : CrudController { }

    // A controller in parts: not read.
    [Route("edges/[controller]")]
    public partial class PartsController : ControllerBase { }
}
