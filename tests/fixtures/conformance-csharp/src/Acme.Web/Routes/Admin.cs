namespace Acme.Web.Routes;

/// A group given as an argument, and one returned.
public static class Admin
{
    public static void Map(RouteGroupBuilder admin)
    {
        admin.MapDelete("/users/{id}", Handlers.Ban);
    }
}

public static class Reports
{
    public static RouteGroupBuilder Group(WebApplication app) => app.MapGroup("/reports");
}
