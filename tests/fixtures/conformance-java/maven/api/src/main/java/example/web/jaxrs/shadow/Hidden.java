package example.web.jaxrs.shadow;

import javax.ws.rs.*;

// The package's own GET hides JAX-RS's: not a method annotation.
@Path("/hidden")
public class Hidden {
    @GET
    public String hidden() { return ""; }

    @POST
    public String visible() { return ""; }
}
