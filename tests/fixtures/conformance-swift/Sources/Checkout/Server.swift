import Vapor

enum Server {
    static func run() async throws {
        let app = try await Application.make(.detect())
        try routes(app)
    }
}
