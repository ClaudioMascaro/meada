import Foundation

// Through a base URL variable, with a fallback and the trailing slash trimmed.
func priceOf(_ sku: String) async throws -> Data {
    let base = (ProcessInfo.processInfo.environment["WEB_URL"] ?? "http://localhost:3000")
        .trimmingCharacters(in: CharacterSet(charactersIn: "/"))
    let (data, _) = try await URLSession.shared.data(from: URL(string: base + "/api/prices/" + sku)!)
    return data
}

// Through the web app's declared host.
func promotions() async throws -> Data {
    let (data, _) = try await URLSession.shared.data(from: URL(string: "https://web.example.com/api/promotions")!)
    return data
}
