// Known outside members: `Hashable` declares `hash(into:)` and `hashValue`, `Codable`
// `encode(to:)` and `init(from:)`, so the type's own overloads are exact, `hash` stays a name
// match and a name nobody declares resolves to nothing.
struct Tag: Hashable, Codable {
    func open() {}
    func open(seat: Int) {}
    func hash(into hasher: inout Hasher) {}

    func use(_ t: Tag) {
        t.open(seat: 1)
        open()
        var hasher = Hasher()
        t.hash(into: &hasher)
        t.missing()
    }
}

// A raw value type and `CaseIterable` declare `rawValue`, `init(rawValue:)` and `allCases`.
enum Status: String, CaseIterable {
    case open, closed

    func label() -> String { "" }
    func label(upper: Bool) -> String { "" }

    func shout() -> String { label(upper: true) }
}
