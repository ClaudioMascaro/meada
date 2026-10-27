protocol Carrier {
    func ship(parcel: Parcel)
}

struct Parcel {
    let weight: Int
}

struct Crate {}

struct Truck: Carrier {
    func ship(parcel: Parcel) {}

    func ship(parcel: Parcel, fast: Bool) {}

    func load(_ parcel: Parcel) {}

    func load(_ crate: Crate) {}
}

struct Boat: Carrier {
    func ship(parcel: Parcel) {}

    func moor() {}
}

final class Depot {
    let truck: Truck
    var carrier: Carrier
    let boat = Boat()

    init(truck: Truck, carrier: Carrier) {
        self.truck = truck
        self.carrier = carrier
    }

    func dispatch(parcel: Parcel, via lorry: Truck, other: Carrier, wrapped: Truck?, ships: [Boat]) {
        truck.ship(parcel: parcel)
        self.truck.ship(parcel: parcel, fast: true)
        lorry.ship(parcel: parcel)
        wrapped?.ship(parcel: parcel)
        let local: Truck = lorry
        local.ship(parcel: parcel)
        let made = Truck()
        made.load(parcel)
        carrier.ship(parcel: parcel)
        other.ship(parcel: parcel)
        boat.moor()
        ships.first?.moor()
        let found = lookup()
        found.ship(parcel: parcel)
        found.ship(parcel: parcel, fast: true)
        found.moor()
    }

    func again(lorry: Truck) {
        let lorry = Boat()
        lorry.ship(parcel: Parcel(weight: 1))
    }

    func lookup() -> Truck {
        return truck
    }
}
