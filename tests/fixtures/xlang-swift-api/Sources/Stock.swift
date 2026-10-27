import Alamofire
import Foundation

// Through a base URL variable read with a force unwrap.
func stock() async throws {
    let base = ProcessInfo.processInfo.environment["STOCK_URL"]!
    _ = try await AF.request("\(base)/api/stock").serializingData().value
}
