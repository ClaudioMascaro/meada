import UIKit

// UIKit re-exports Foundation: its import alone spells `URLSession`.
func uikit(session: URLSession) async throws {
    _ = try await session.data(from: URL(string: "https://api.example.com/uikit")!)
}

func reroute() { gateHost = "https://other.example.com" }
