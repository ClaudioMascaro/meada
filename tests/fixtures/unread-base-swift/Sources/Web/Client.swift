import Foundation

func loadBase() -> String {
    ProcessInfo.processInfo.environment["API_URL"]! + "/v2"
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
