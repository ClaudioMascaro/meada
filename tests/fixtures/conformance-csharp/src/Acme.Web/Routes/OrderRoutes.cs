namespace Acme.Web.Routes;

/// Routes mapped on whichever group calls them: each route is served under every prefix.
public static class OrderRoutes
{
    public static RouteGroupBuilder MapOrders(this RouteGroupBuilder group)
    {
        group.MapGet("/", () => Store.All());
        group.MapGet("/{id:int}", Handlers.Find);
        group.MapDelete("/{id}", (int id) => Store.Remove(id));
        return group;
    }

    /// A group mounted inside itself adds no prefix beyond the first time round.
    public static void Nest(this RouteGroupBuilder group)
    {
        group.MapGet("/leaf", () => "leaf");
        group.MapGroup("/again").Nest();
    }
}
