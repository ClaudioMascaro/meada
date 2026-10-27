extension Item {
    func display() -> String {
        return label()
    }
}

extension Item.Size {
    func doubled() -> Int {
        return scaled(by: 2)
    }
}

func render(_ size: Item.Size) -> String {
    return ""
}
