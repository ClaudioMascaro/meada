import Alamofire
import AsyncHTTPClient
import Foundation
import Store

// URLSession, Alamofire and AsyncHTTPClient consumers (spec #29, #121).
let billing = "https://billing.example.com"

enum Remote {
    static let base = "https://api.example.com"
    static let users = "\(base)/users"
    static let health = base + "/health"
}

// `data(from:)` is a `GET`; `data(for:)` sends the method its request is assigned.
func sessions(id: Int, session: URLSession) async throws {
    _ = try await URLSession.shared.data(from: URL(string: "https://api.example.com/ping")!)
    let users = URL(string: Remote.users)!
    _ = try await URLSession.shared.data(from: users)
    _ = try await session.data(from: URL(string: "\(Remote.users)/\(id)")!)
    _ = try await session.data(from: URL(string: Remote.health)!)
    var create = URLRequest(url: URL(string: "\(Endpoints.orders)/new")!)
    create.httpMethod = "POST"
    _ = try await URLSession.shared.data(for: create)
    let status = URLRequest(url: URL(string: "/status", relativeTo: URL(string: billing))!)
    _ = try await session.data(for: status)
    let own = URLSession(configuration: .default)
    _ = try await own.data(from: URL(string: "https://api.example.com/own")!)
    let items = URL(string: Remote.base)!.appendingPathComponent("items")
        .appendingPathComponent("\(id)")
    _ = try await URLSession.shared.data(from: items)
}

func alamofire(id: Int, session: Session) {
    _ = AF.request("https://api.example.com/af")
    _ = AF.request("\(Remote.base)/af/\(id)", method: .post)
    _ = session.request(Remote.users, method: .delete)
    _ = AF.request(URL(string: Remote.health)!, method: HTTPMethod.put)
    _ = Session.default.request("https://api.example.com/default", method: .patch)
}

func asyncHTTP(client: HTTPClient) async throws {
    var post = HTTPClientRequest(url: "\(Remote.base)/ahc")
    post.method = .POST
    _ = try await client.execute(post, timeout: .seconds(5))
    let get = HTTPClientRequest(url: Remote.health)
    _ = try await HTTPClient.shared.execute(get, timeout: .seconds(5))
}

// A wrapper takes the path or the method from its callers, through any number of levels.
struct Api {
    let session = URLSession.shared

    func fetch(_ path: String) async throws -> Data {
        let (data, response) = try await session.data(from: URL(string: Remote.base + path)!)
        if (response as? HTTPURLResponse)?.statusCode == 503 { return try await fetch(path) }
        return data
    }

    func user(_ id: Int) async throws -> Data {
        try await fetch("/users/\(id)")
    }

    func profile() async throws -> Data {
        try await user(7)
    }

    func send(_ method: String, _ path: String) async throws {
        var request = URLRequest(url: URL(string: Remote.base + path)!)
        request.httpMethod = method
        _ = try await session.data(for: request)
    }

    func remove(_ id: Int) async throws {
        try await send("DELETE", "/users/\(id)")
    }

    // A parameter or a local of the name hides the property.
    func cached(session: Cache) { _ = session.data(from: "k") }

    func local() {
        let session = Cache()
        _ = session.data(from: "k")
    }
}

final class Gateway {
    let session: URLSession

    init(session: URLSession) { self.session = session }

    func ping() async throws {
        _ = try await self.session.data(from: URL(string: "https://api.example.com/gateway")!)
    }

    func pong() async throws {
        _ = try await session.data(from: URL(string: "https://api.example.com/pong")!)
    }
}

// A call matched by name alone is no call of the wrapper.
func byName(other: Mystery) async throws {
    _ = try await other.fetch("/inferred")
}

// A client assigned again holds no one value.
func moved(other: URLSession) async throws {
    var client = URLSession.shared
    client = other
    _ = try await client.data(from: URL(string: "https://api.example.com/client")!)
}

func useApi() async throws {
    let api = Api()
    _ = try await api.fetch("/direct")
    try await api.remove(3)
}

// A URL that cannot be evaluated is an unresolved consumer.
func unresolved(path: String, flag: Bool) async throws {
    _ = try await URLSession.shared.data(from: URL(string: lookup())!)
    _ = try await URLSession.shared.data(from: URL(string: path)!)
    var moved = URLRequest(url: URL(string: "https://api.example.com/first")!)
    moved = URLRequest(url: URL(string: "https://api.example.com/second")!)
    _ = try await URLSession.shared.data(for: moved)
    var maybe = URLRequest(url: URL(string: "https://api.example.com/maybe")!)
    if flag { maybe.httpMethod = "PUT" }
    _ = try await URLSession.shared.data(for: maybe)
    var twice = URLRequest(url: URL(string: "https://api.example.com/twice")!)
    twice.httpMethod = "PUT"
    twice.httpMethod = "DELETE"
    _ = try await URLSession.shared.data(for: twice)
}

func lookup() -> String { "https://api.example.com/lookup" }

struct Cache {
    func data(from key: String) -> Data? { nil }
}

struct Local {
    func request(_ url: String) {}
}

// Not clients of the libraries: `dataTask(with:)` is not read, a type of the repository is no
// session, and a local named `AF` hides Alamofire's.
func notRead(session: URLSession, cache: Cache) {
    session.dataTask(with: URL(string: "https://api.example.com/task")!) { _, _, _ in }.resume()
    _ = cache.data(from: "key")
    let AF = Local()
    AF.request("https://api.example.com/hidden")
}
