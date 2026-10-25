package example.web.jaxrs;

import javax.ws.rs.DELETE;
import javax.ws.rs.GET;
import javax.ws.rs.HEAD;
import javax.ws.rs.OPTIONS;
import javax.ws.rs.PATCH;
import javax.ws.rs.POST;
import javax.ws.rs.PUT;
import javax.ws.rs.Path;
import javax.ws.rs.PathParam;

@Path("/items")
public class Items {
    @GET
    public String list() { return ""; }

    @GET
    @Path("{id}")
    public String get(@PathParam("id") String id) { return id; }

    @POST
    @Path("/new")
    public String create() { return ""; }

    @PUT
    @Path("/{id:[0-9]+}")
    public String replace() { return ""; }

    @PATCH
    @Path("/{id}/flag")
    public String flag() { return ""; }

    @DELETE
    @Path("{id}")
    public void remove() {}

    @HEAD
    public void probe() {}

    @OPTIONS
    public void options() {}

    // A @Path without a method annotation is a sub-resource locator, not a route.
    @Path("/locator")
    public Object locator() { return null; }
}
