@testable import App

func check() -> Int {
    let built = buildApplication()
    let hidden = secret()
    return built + hidden
}
