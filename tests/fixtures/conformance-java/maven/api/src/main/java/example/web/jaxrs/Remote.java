package example.web.jaxrs;

import javax.ws.rs.GET;
import javax.ws.rs.Path;

// A REST client interface (MicroProfile, Quarkus) is no server: it gives no provider.
@Path("/remote")
public interface Remote {
    @GET
    String fetch();
}
