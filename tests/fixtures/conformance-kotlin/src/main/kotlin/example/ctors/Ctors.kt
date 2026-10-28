package example.ctors

class Parcel internal constructor(val url: String) {
    constructor(url: String, method: String) : this(url)

    open class Maker {
        constructor() {}
        internal constructor(parcel: Parcel) {}

        fun assemble(): Parcel = Parcel("x")
    }
}

class Plain(val body: String = "")

class Two {
    constructor(body: String) {}
    constructor() {}
}

class Same {
    constructor(a: Int) {}
    constructor(a: String) {}
}

fun construct() {
    Plain(body = "x")
    Two(body = "x")
    Two()
    Parcel("u")
    Parcel("u", "GET")
    Parcel.Maker()
    Parcel.Maker().assemble()
    Same(1)
}
