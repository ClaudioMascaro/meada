import Foundation
import Vapor

// A top-level constant and a type-level one bound to a read.
let packageBase = ProcessInfo.processInfo.environment["PKG_URL"] ?? ""

enum Config {
    static let base = ProcessInfo.processInfo.environment["STATIC_URL"] ?? ""
}

func defaultBase() -> String { "http://localhost:3000" }

let defaultURL = "http://localhost:4000"

// Reads: each function's request goes to the base URL variable named in the call.

func direct() async throws {
    let (_, _) = try await URLSession.shared.data(from: URL(string: ProcessInfo.processInfo.environment["DIRECT_URL"]! + "/a")!)
}

func named() async throws {
    let base = ProcessInfo.processInfo.environment["NAMED_URL"]!
    let (_, _) = try await URLSession.shared.data(from: URL(string: base + "/a")!)
}

func fallback() async throws {
    let base = ProcessInfo.processInfo.environment["FALLBACK_URL"] ?? "http://localhost:3000"
    let (_, _) = try await URLSession.shared.data(from: URL(string: "\(base)/a")!)
}

func emptyFallback() async throws {
    let base = ProcessInfo.processInfo.environment["EMPTY_URL"] ?? ""
    let (_, _) = try await URLSession.shared.data(from: URL(string: "\(base)/a")!)
}

func trimmed() async throws {
    let base = (ProcessInfo.processInfo.environment["TRIMMED_URL"] ?? "")
        .trimmingCharacters(in: CharacterSet(charactersIn: "/"))
    let (_, _) = try await URLSession.shared.data(from: URL(string: base + "/a")!)
}

func optionalTrim() async throws {
    let base = ProcessInfo.processInfo.environment["OPTIONAL_URL"]?.trimmingCharacters(in: .whitespaces) ?? ""
    let (_, _) = try await URLSession.shared.data(from: URL(string: base + "/a")!)
}

func replaced() async throws {
    let base = ProcessInfo.processInfo.environment["REPLACED_URL"]!.replacingOccurrences(of: " ", with: "")
    let (_, _) = try await URLSession.shared.data(from: URL(string: base + "/a")!)
}

func lowered() async throws {
    let (_, _) = try await URLSession.shared.data(from: URL(string: ProcessInfo.processInfo.environment["LOWERED_URL"]!.lowercased() + "/a")!)
}

func qualified() async throws {
    let (_, _) = try await URLSession.shared.data(from: URL(string: (Foundation.ProcessInfo.processInfo.environment["QUALIFIED_URL"] ?? "") + "/a")!)
}

func interpolated() async throws {
    let (_, _) = try await URLSession.shared.data(from: URL(string: "\(ProcessInfo.processInfo.environment["INTERPOLATED_URL"] ?? "")/a")!)
}

func topLevel() async throws {
    let (_, _) = try await URLSession.shared.data(from: URL(string: packageBase + "/a")!)
}

func typeLevel() async throws {
    let (_, _) = try await URLSession.shared.data(from: URL(string: Config.base + "/a")!)
}

// Near misses: more or other than exactly one read of the process environment is no base.

func fallbackToAnotherRead() async throws {
    let base = ProcessInfo.processInfo.environment["FIRST_URL"] ?? ProcessInfo.processInfo.environment["SECOND_URL"] ?? "x"
    let (_, _) = try await URLSession.shared.data(from: URL(string: base + "/a")!)
}

func fallbackToACall() async throws {
    let base = ProcessInfo.processInfo.environment["CALL_URL"] ?? defaultBase()
    let (_, _) = try await URLSession.shared.data(from: URL(string: base + "/a")!)
}

func fallbackToAName() async throws {
    let base = ProcessInfo.processInfo.environment["NAME_URL"] ?? defaultURL
    let (_, _) = try await URLSession.shared.data(from: URL(string: base + "/a")!)
}

func fallbackInterpolated() async throws {
    let host = "localhost"
    let base = ProcessInfo.processInfo.environment["HOST_URL"] ?? "http://\(host)"
    let (_, _) = try await URLSession.shared.data(from: URL(string: base + "/a")!)
}

func trimmedByARead() async throws {
    let base = ProcessInfo.processInfo.environment["TRIM_URL"]!
        .replacingOccurrences(of: ProcessInfo.processInfo.environment["OTHER"]!, with: "")
    let (_, _) = try await URLSession.shared.data(from: URL(string: base + "/a")!)
}

func nameFromAVariable() async throws {
    let key = "KEY_URL"
    let (_, _) = try await URLSession.shared.data(from: URL(string: ProcessInfo.processInfo.environment[key]! + "/a")!)
}

func methodOfAnotherKind() async throws {
    let base = ProcessInfo.processInfo.environment["KIND_URL"]!.appending("/b")
    let (_, _) = try await URLSession.shared.data(from: URL(string: base + "/a")!)
}

func assignedAgain() async throws {
    var again = ProcessInfo.processInfo.environment["AGAIN_URL"]!
    again = "http://localhost:3000"
    let (_, _) = try await URLSession.shared.data(from: URL(string: again + "/a")!)
}

func vaporEnvironment() async throws {
    let (_, _) = try await URLSession.shared.data(from: URL(string: (Environment.get("VAPOR_URL") ?? "") + "/a")!)
}

func ofAnotherObject() async throws {
    let (_, _) = try await URLSession.shared.data(from: URL(string: settings.environment["SETTINGS_URL"]! + "/a")!)
}

func optionalBinding() async throws {
    if let base = ProcessInfo.processInfo.environment["BINDING_URL"] {
        let (_, _) = try await URLSession.shared.data(from: URL(string: base + "/a")!)
    }
}
