package example.chains

class Lid {
    fun latch() {}
}

class Box {
    val lid: Lid = Lid()

    fun unlatch(): Lid = Lid()
}

fun Box.stamp() {}

fun Lid.stamp() {}

class Epoch {
    fun toEpochMilli(): Long = 0
}

class Crate<T> {
    fun peek(): Lid = Lid()
}

fun make(): Box = Box()

fun crate(): Crate<Int> = Crate()

fun maker(): () -> Box = ::make

fun find(): Box? = null

fun g() = Box()

fun <T> pick(): T = TODO()

fun now(): java.time.Instant = TODO()

fun use() {
    make().unlatch().latch()
    Box().unlatch()
    find()?.unlatch()
    g().unlatch()
    pick<Box>().unlatch()
    now().toEpochMilli()
    make()!!.unlatch()
    make().lid.stamp()
    crate().peek()
    maker().unlatch()
}
