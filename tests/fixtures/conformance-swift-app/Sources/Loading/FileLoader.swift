final class FileLoader: Loader {
    func load(from path: String) -> [Item] {
        let item = Item(name: path)
        return [item]
    }

    func load(from path: String, limit: Int) -> [Item] {
        let all = load(from: path)
        report { text in
            print(text)
        }
        return Array(all.prefix(limit))
    }

    func report(_ format: (String) -> Void) {
        format(render(Item(name: "x")))
        let render = { (item: Item) in item.display() }
        _ = render(Item(name: "y"))
    }
}
