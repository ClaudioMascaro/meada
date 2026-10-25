package example.web.jaxrs;

import javax.ws.rs.ApplicationPath;
import javax.ws.rs.GET;
import javax.ws.rs.Path;
import javax.ws.rs.core.Application;

// @ApplicationPath is not applied: the route below stays "/app/alive".
@ApplicationPath("/rest")
public class App extends Application {
}

@Path("/app")
class Ping {
    @GET
    @Path("/alive")
    public String alive() { return ""; }
}
