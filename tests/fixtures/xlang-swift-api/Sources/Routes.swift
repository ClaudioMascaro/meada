import Vapor

func routes(_ app: Application) throws {
    app.get("orders", ":id", use: status)
    app.post("orders", "reserve", use: reserve)
}
