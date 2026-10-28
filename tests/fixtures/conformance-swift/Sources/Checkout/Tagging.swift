import Store

// A type's overloads merge across modules: an extension here adds one, and a call that fits the
// type's own overload in `Store` still reaches it.
extension Tag {
    func open(label: String) {}
}

func tagging(_ tag: Tag) {
    tag.open(seat: 1)
}
