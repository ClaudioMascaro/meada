func outside(_ name: String, _ loader: Loader, _ file: FileLoader, _ size: Item.Size) {
    name.label()
    loader.load(from: "x")
    file.load(from: "x")
    file.load(from: "x", limit: 1)
    size.scaled(by: 2)
}

func hidden(_ item: Item, _ items: [Item]) {
    item.label()
    items.forEach { item in
        item.display()
    }
}

func rebound(_ item: Item?) {
    guard let item = item else { return }
    item.label()
}

func typed(_ item: Item) {
    let loader = FileLoader()
    loader.report { _ in }
    item.label()
}

func generic<Item>(_ item: Item) {
    item.label()
}
