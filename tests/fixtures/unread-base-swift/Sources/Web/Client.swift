import Foundation

func loadBase() -> String {
    CommandLine.arguments[1] + "/v2"
}

func viaCall() async throws {
    _ = try await URLSession.shared.data(from: URL(string: loadBase() + "/orders")!)
}

func viaComponent() async throws {
    let base = URL(string: loadBase())!
    _ = try await URLSession.shared.data(from: base.appendingPathComponent("items"))
}

func viaRelative() async throws {
    _ = try await URLSession.shared.data(from: URL(string: "/orders", relativeTo: URL(string: loadBase())!)!)
}
