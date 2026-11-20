import org.springframework.web.client.RestTemplate;

class Client {
    private final RestTemplate rest = new RestTemplate();
    private String javaMethodBase() { return System.getenv("API_URL"); }
    private static String javaFunctionBase() { return System.getenv("API_URL") + "/v2"; }
    void ViaMethod() { rest.getForObject(javaMethodBase() + "/api/orders", String.class); }
    void ViaFunction() { rest.getForObject(javaFunctionBase() + "/orders", String.class); }
    void envGet(String key, String path) { rest.getForObject(System.getenv(key) + path, String.class); }
    void ParamA() { envGet("API_URL", "/api/orders"); }
    void ParamB() { envGet("SECOND_URL", "/api/orders"); }
    void ParamUnknown(String key) { envGet(key, "/api/orders"); }
    void QueryAppend(boolean flag) {
        String path = "/api/orders";
        if (flag) { path += "?x=1"; }
        rest.getForObject(System.getenv("API_URL") + path, String.class);
    }
    void QueryRewrite(boolean flag) {
        String path = "/api/orders";
        if (flag) { path = path + "?x=1"; }
        rest.getForObject(System.getenv("API_URL") + path, String.class);
    }
    void QuerySuffix(boolean flag) {
        String q = "";
        if (flag) { q = "?x=1"; }
        rest.getForObject(System.getenv("API_URL") + "/api/orders" + q, String.class);
    }
    void QueryConditional(boolean flag) {
        rest.getForObject(System.getenv("API_URL") + "/api/orders" + (flag ? "?x=1" : ""), String.class);
    }
}
