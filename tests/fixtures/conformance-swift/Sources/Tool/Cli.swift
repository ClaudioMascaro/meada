import Store

// A type marked `@main` starts a program: in a target of its own, since a module with a
// `main.swift` has its top-level code as its entry point.
@main
struct Cli {
    static func main() {
        log(level: 1)
    }
}
