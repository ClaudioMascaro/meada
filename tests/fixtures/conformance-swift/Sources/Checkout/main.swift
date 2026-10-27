import Vapor

let app = try await Application.make(.detect())
app.get("started") { req in "started" }
let edge = app.grouped("edge")
edge.get("unread") { req in "unread" }
for edge in [app] {
    edge.get("unread") { req in "unread" }
}
summarize()
