struct Ship {
    func place() {}
}

struct Store {
    func place() {}
}

class Base {
    var store = Store()
}

class Sub: Base {
    var summary: String {
        let store: Ship = Ship()
        store.place()
        return ""
    }

    func go() {
        store.place()
    }
}

class Gone {
    var store: Store

    init(store: Store) {
        self.store = store
    }

    deinit {
        let store: Ship = Ship()
        store.place()
    }

    func go() {
        store.place()
    }
}

class Desk {
    var ship: Ship = Ship()

    func submit(_ ready: Bool) {
        if ready {
            let ship: Store = Store()
            ship.place()
        }
        ship.place()
    }
}

struct Page<Item> {
    func own() {
        _ = Item()
    }
}

protocol Shelf {
    associatedtype Item
}

extension Shelf {
    func stock(_ item: Item) {
        item.label()
    }
}

func make<Item>() {
    let item = Item()
    item.label()
}
