import Hummingbird

// Hummingbird routes: a router's groups nest, take the router through arguments and return
// values, and `on(path, method:)` names its method after the path.
func gateway() throws {
    let router = Router()
    router.get("health") { _, _ in "ok" }
    let v1 = router.group("v1")
    v1.get("items/:id", use: showItem)
    v1.post("items") { request, context in "created" }
    v1.put("items/{id}") { _, _ in "replaced" }
    v1.patch("items/:id/name") { _, _ in "renamed" }
    v1.delete("files/**") { _, _ in "gone" }
    v1.on("search", method: .get) { _, _ in "found" }
    let admin = v1.group("admin").group("tools")
    admin.get("ping") { _, _ in "pong" }
    mountMetrics(router.group("internal"))
    let shared = deeper(admin)
    shared.get("hammer") { _, _ in "tool" }
    router.group().get("bare") { _, _ in "bare" }
    router.group(String(describing: router)).get("unread") { _, _ in "unread" }
    router.get("a/b") { _, _ in "nested" }
}

func mountMetrics(_ routes: Router<BasicRequestContext>) {
    routes.get("metrics") { _, _ in "metrics" }
}

func deeper(_ parent: RouterGroup<BasicRequestContext>) -> RouterGroup<BasicRequestContext> {
    parent.group("deeper")
}

@Sendable func showItem(request: Request, context: BasicRequestContext) async throws -> String {
    "item"
}
