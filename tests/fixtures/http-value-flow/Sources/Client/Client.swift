import Foundation

func functionBase() -> String {
    ProcessInfo.processInfo.environment["API_URL"]! + "/v2"
}

struct Client {
    func methodBase() -> String { ProcessInfo.processInfo.environment["API_URL"]! }
    func ViaMethod() async throws {
        _ = try await URLSession.shared.data(from: URL(string: methodBase() + "/api/orders")!)
    }
    func methodPrefix() -> String { ProcessInfo.processInfo.environment["API_URL"]! + "/v2" }
    func MethodWithPrefix() async throws {
        _ = try await URLSession.shared.data(from: URL(string: methodPrefix() + "/orders")!)
    }
}

func ViaFunction() async throws {
    _ = try await URLSession.shared.data(from: URL(string: functionBase() + "/orders")!)
}

func plainBase() -> String { ProcessInfo.processInfo.environment["API_URL"]! }

func FunctionWithoutPrefix() async throws {
    _ = try await URLSession.shared.data(from: URL(string: plainBase() + "/api/orders")!)
}

func envGet(_ key: String, _ path: String) async throws {
    _ = try await URLSession.shared.data(from: URL(string: ProcessInfo.processInfo.environment[key]! + path)!)
}

func ParamA() async throws { try await envGet("API_URL", "/api/orders") }
func ParamB() async throws { try await envGet("SECOND_URL", "/api/orders") }
func ParamUnknown(_ key: String) async throws { try await envGet(key, "/api/orders") }

func QueryAppend(_ flag: Bool) async throws {
    var path = "/api/orders"
    if flag { path += "?x=1" }
    _ = try await URLSession.shared.data(from: URL(string: ProcessInfo.processInfo.environment["API_URL"]! + path)!)
}

func QueryRewrite(_ flag: Bool) async throws {
    var path = "/api/orders"
    if flag { path = path + "?x=1" }
    _ = try await URLSession.shared.data(from: URL(string: ProcessInfo.processInfo.environment["API_URL"]! + path)!)
}

func QuerySuffix(_ flag: Bool) async throws {
    var query = ""
    if flag { query = "?x=1" }
    _ = try await URLSession.shared.data(from: URL(string: ProcessInfo.processInfo.environment["API_URL"]! + "/api/orders" + query)!)
}

func QueryConditional(_ flag: Bool) async throws {
    let url = ProcessInfo.processInfo.environment["API_URL"]! + "/api/orders" + (flag ? "?x=1" : "")
    _ = try await URLSession.shared.data(from: URL(string: url)!)
}
