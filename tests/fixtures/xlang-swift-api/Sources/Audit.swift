import Foundation

// Through a base URL variable, in a request.
func audit() async throws {
    let base = ProcessInfo.processInfo.environment["WEB_URL"] ?? ""
    var request = URLRequest(url: URL(string: "\(base)/api/audit")!)
    request.httpMethod = "POST"
    _ = try await URLSession.shared.data(for: request)
}
