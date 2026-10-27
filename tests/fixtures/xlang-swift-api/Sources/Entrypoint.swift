import Vapor

@main
enum Entrypoint {
    static func main() async throws {
        let app = try await Application.make(.detect())
        try await configure(app)
        try await app.execute()
    }
}

func configure(_ app: Application) async throws {
    try routes(app)
}
