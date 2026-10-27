import Vapor

// Vapor routes: groups chain, a RouteCollection registers under a group, and a builder reaches
// a function through its argument and its return value.
func routes(_ app: Application) throws {
    app.get { req in "root" }
    let api = app.grouped("api", "v1")
    api.get("users", ":id", use: showUser)
    api.post("users") { req async throws -> String in
        return "created"
    }
    api.on(.PATCH, "users/:id/name") { req in "renamed" }
    api.put("assets", "*") { req in "asset" }
    api.delete("assets", "**") { req in "gone" }
    let tools = api.grouped("admin").grouped("tools")
    tools.get("ping") { req in "pong" }
    try api.register(collection: OrderController())
    try api.grouped("beta").register(collection: OrderController())
    mountReports(api)
    mountReports(app.grouped("internal"))
    mountAudit(under: api.grouped("audit"))
    let shared = sharedTools(api)
    shared.get("hammer") { req in "tool" }
    nest(app.grouped("loop"))
    app.group("closure") { group in
        group.get("skipped") { req in "not read" }
    }
}

func mountReports(_ routes: RoutesBuilder) {
    routes.get("reports", use: showReport)
}

func mountAudit(under routes: RoutesBuilder) {
    routes.get("log") { req in "audit" }
}

// A group mounted inside itself adds nothing more.
func nest(_ routes: RoutesBuilder) {
    routes.get("leaf") { req in "leaf" }
    nest(routes.grouped("again"))
}

func sharedTools(_ parent: RoutesBuilder) -> RoutesBuilder {
    parent.grouped("shared")
}

func showUser(req: Request) throws -> String {
    "user"
}

func showReport(req: Request) -> String {
    "report"
}

struct OrderController: RouteCollection {
    func boot(routes: RoutesBuilder) throws {
        let orders = routes.grouped("orders")
        orders.get(use: list)
        orders.get(":id", use: self.show)
        orders.post { req in "placed" }
    }

    func list(req: Request) -> [String] {
        []
    }

    func show(req: Request) -> String {
        "order"
    }
}

// No call reaches it: its builder comes from nowhere.
func orphan(_ routes: RoutesBuilder) {
    routes.get("orphan") { req in "nobody" }
}
