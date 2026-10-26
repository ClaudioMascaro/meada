using Acme.Core.Text;
using static Acme.Core.Text.Format;
using static Acme.Core.Orders.Order;
using Acme.Contracts;
using Acme.Hosting;
using Text = Acme.Core.Text;
using Svc = Acme.Core.Orders.OrderService;

namespace Acme.Api
{
    public class Checkout
    {
        private readonly OrderService orders = new OrderService("eu");

        public void Run()
        {
            Upper("x");
            Text.Format.Upper("y");
            Svc.Default();
            Acme.Core.Text.Casing.Lower("z");
            OrderService.Default();
            // `using static` of a partial type reaches every part.
            Create();
            // One name in two projects is two types, `partial` or not.
            new OrderDto();
            new Shell();
            // Acme.Core's global usings stop at its project.
            Lower("w");
            Fmt.Upper("v");
        }
    }
}
