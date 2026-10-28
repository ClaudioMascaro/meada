package example.chains

class Lid {
    fun latch() {}
}

class Box {
    fun unlatch(): Lid = Lid()
}

class Epoch {
    fun toEpochMilli(): Long = 0
}

fun make(): Box = Box()

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
}
