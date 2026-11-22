using System;
using System.Net.Http;

class Client {
    private readonly HttpClient http = new HttpClient();
    private string MethodBase() => Environment.GetEnvironmentVariable("API_URL")!;
    private static string FunctionBase() => Environment.GetEnvironmentVariable("API_URL") + "/v2";
    void ViaMethod() { _ = http.GetAsync(MethodBase() + "/api/orders"); }
    void ViaFunction() { _ = http.GetAsync(FunctionBase() + "/orders"); }
    private string MethodPrefix() => Environment.GetEnvironmentVariable("API_URL") + "/v2";
    private static string PlainBase() => Environment.GetEnvironmentVariable("API_URL")!;
    void MethodWithPrefix() { _ = http.GetAsync(MethodPrefix() + "/orders"); }
    void FunctionWithoutPrefix() { _ = http.GetAsync(PlainBase() + "/api/orders"); }
    void EnvGet(string key, string path) { _ = http.GetAsync(Environment.GetEnvironmentVariable(key) + path); }
    void ParamA() { EnvGet("API_URL", "/api/orders"); }
    void ParamB() { EnvGet("SECOND_URL", "/api/orders"); }
    void ParamUnknown(string key) { EnvGet(key, "/api/orders"); }
    void QueryAppend(bool flag) {
        var path = "/api/orders";
        if (flag) { path += "?x=1"; }
        _ = http.GetAsync(Environment.GetEnvironmentVariable("API_URL") + path);
    }
    void QueryRewrite(bool flag) {
        var path = "/api/orders";
        if (flag) { path = path + "?x=1"; }
        _ = http.GetAsync(Environment.GetEnvironmentVariable("API_URL") + path);
    }
    void QuerySuffix(bool flag) {
        var query = "";
        if (flag) { query = "?x=1"; }
        _ = http.GetAsync(Environment.GetEnvironmentVariable("API_URL") + "/api/orders" + query);
    }
    void QueryConditional(bool flag) {
        _ = http.GetAsync(Environment.GetEnvironmentVariable("API_URL") + "/api/orders" + (flag ? "?x=1" : ""));
    }
}
