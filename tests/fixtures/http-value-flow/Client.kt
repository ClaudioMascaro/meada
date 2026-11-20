import org.springframework.web.client.RestTemplate

private fun kotlinFunctionBase(): String = System.getenv("API_URL") + "/v2"
private fun kotlinPlainBase(): String = System.getenv("API_URL")

class Client(private val rest: RestTemplate) {
    private fun kotlinMethodBase(): String { return System.getenv("API_URL") }
    fun ViaMethod() { rest.getForObject(kotlinMethodBase() + "/api/orders", String::class.java) }
    fun ViaFunction() { rest.getForObject(kotlinFunctionBase() + "/orders", String::class.java) }
    private fun kotlinMethodPrefix(): String = System.getenv("API_URL") + "/v2"
    fun MethodWithPrefix() { rest.getForObject(kotlinMethodPrefix() + "/orders", String::class.java) }
    fun FunctionWithoutPrefix() { rest.getForObject(kotlinPlainBase() + "/api/orders", String::class.java) }
    private fun envGet(key: String, path: String) { rest.getForObject(System.getenv(key) + path, String::class.java) }
    fun ParamA() { envGet("API_URL", "/api/orders") }
    fun ParamB() { envGet("SECOND_URL", "/api/orders") }
    fun ParamUnknown(key: String) { envGet(key, "/api/orders") }
    fun QueryAppend(flag: Boolean) {
        var path = "/api/orders"
        if (flag) { path += "?x=1" }
        rest.getForObject(System.getenv("API_URL") + path, String::class.java)
    }
    fun QueryRewrite(flag: Boolean) {
        var path = "/api/orders"
        if (flag) { path = path + "?x=1" }
        rest.getForObject(System.getenv("API_URL") + path, String::class.java)
    }
    fun QuerySuffix(flag: Boolean) {
        var q = ""
        if (flag) { q = "?x=1" }
        rest.getForObject(System.getenv("API_URL") + "/api/orders" + q, String::class.java)
    }
    fun QueryConditional(flag: Boolean) {
        rest.getForObject(System.getenv("API_URL") + "/api/orders" + (if (flag) "?x=1" else ""), String::class.java)
    }
}
