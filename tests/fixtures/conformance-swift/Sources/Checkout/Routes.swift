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
    mount(admin: app.grouped("admin"), app.grouped("public"))
    secured(on: api.grouped("sec")) { _ in }
    explicit(api).get("explicit") { req in "explicit" }
    spin(app.grouped("spin")).get("never") { req in "unread" }
    pick(app)
    pick(spin(app))
    app.grouped(AuthMiddleware()).get("guarded") { req in "guarded" }
    app.grouped(PathComponent("typed")).get("unread") { req in "unread" }
    app.grouped(String(describing: app)).get("unread") { req in "unread" }
    shadowedByLet(app)
    shadowedByLoop(app)
    shadowedByIf(app, nil)
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
        unlisted(routes)
        let alias = routes
        alias.get("alias") { req in "alias" }
    }

    func list(req: Request) -> [String] {
        []
    }

    func show(req: Request) -> String {
        "order"
    }
}

// A call made inside a type of an outside protocol is a name match (`RouteCollection` may
// declare the helper), so what `boot` passes it is not followed.
func unlisted(_ routes: RoutesBuilder) {
    routes.get("unlisted") { req in "unlisted" }
}

// No call reaches it: its builder comes from nowhere.
func orphan(_ routes: RoutesBuilder) {
    routes.get("orphan") { req in "nobody" }
}

// A parameter takes the argument of its own label, whatever comes first, and the trailing
// closure belongs to no parameter of the builder.
func mount(admin: RoutesBuilder, _ open: RoutesBuilder) {
    admin.get("stats") { req in "stats" }
}

func secured(on routes: RoutesBuilder, _ build: (RoutesBuilder) -> Void) {
    routes.get("secured") { req in "secured" }
}

// An explicit return hands the builder back.
func explicit(_ parent: RoutesBuilder) -> RoutesBuilder {
    let child = parent.grouped("ex")
    return child
}

// A factory that calls itself to make its builder is read nowhere.
func spin(_ parent: RoutesBuilder) -> RoutesBuilder {
    spin(parent).grouped("again")
}

// A route under a builder one of whose origins went round a factory calling itself is served
// under the other origin alone.
func pick(_ builder: RoutesBuilder) {
    builder.grouped("pick").get("one") { req in "unread" }
}

// A name bound again in the function is unknown there, so nothing it holds is served.
func shadowedByLet(_ app: Application) {
    let v1 = app.grouped("v1")
    do {
        let v1 = app
        v1.get("unread") { req in "unread" }
    }
    v1.get("unread") { req in "unread" }
}

func shadowedByLoop(_ app: Application) {
    let v2 = app.grouped("v2")
    for v2 in [app] {
        v2.get("unread") { req in "unread" }
    }
}

func shadowedByIf(_ app: Application, _ other: Application?) {
    let v3 = app.grouped("v3")
    if let v3 = other {
        v3.get("unread") { req in "unread" }
    }
}
