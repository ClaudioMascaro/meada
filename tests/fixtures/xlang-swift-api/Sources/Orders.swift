import Vapor

func status(_ req: Request) async throws -> String {
    try await priceOf(req.parameters.get("id") ?? "")
    try await stock()
    return "ok"
}

func reserve(_ req: Request) async throws -> HTTPStatus {
    try await promotions()
    try await audit()
    return .accepted
}
