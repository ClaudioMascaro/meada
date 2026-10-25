package example.web.jaxrs;

import example.web.Api;
import jakarta.ws.rs.*;

@Path(Api.BASE + "/jakarta")
public class Jakarta {
    @GET
    @Path(Api.ITEM)
    public String get() { return ""; }

    @POST
    public String create() { return ""; }
}
