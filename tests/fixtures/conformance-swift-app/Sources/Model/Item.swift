import Foundation

struct Item {
    let name: String

    func label() -> String {
        return name
    }

    enum Size {
        case small, large

        func scaled(by factor: Int) -> Int {
            return factor
        }
    }
}

protocol Loader {
    func load(from path: String) -> [Item]
}

func render(_ item: Item) -> String {
    return item.label()
}
