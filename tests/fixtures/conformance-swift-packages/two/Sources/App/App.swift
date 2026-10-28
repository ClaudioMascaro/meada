internal func buildApplication() -> Int {
    return 1
}

fileprivate func secret() -> Int {
    return 2
}

func start() -> Int {
    let built = buildApplication()
    let hidden = secret()
    return built + hidden
}
