package example

import com.outside.Outside
import example.ext.describe
import example.ext.tag

// Typed receivers, extension functions and scope functions (spec #27, #96).
interface Store {
    fun get(key: String): String

    fun put(key: String, value: String)
}

open class Cache : Store {
    override fun get(key: String): String = key

    override fun put(key: String, value: String) {}

    fun size(): Int = 0

    fun size(extra: Int): Int = extra

    fun clear() {}

    fun wipe() {}

    fun add(item: String) {}

    fun add(item: Int) {}
}

class Disk(val cache: Cache, private val store: Store, plain: Cache) : Cache() {
    private val spare = Cache()
    private val boxed: Cache? = null
    private val made = build()

    init {
        plain.wipe()
    }

    fun flush() {
        cache.clear()
        store.put("a", "b")
        spare.size()
        spare.size(1)
        boxed?.wipe()
        this.cache.size(2)
        made.wipe()
    }

    fun inherited(): Int = this.size()
}

fun build(): Cache = Cache()

fun parameter(cache: Cache, store: Store, disk: Disk) {
    cache.size()
    cache.size(1, 2)
    cache.add(1)
    store.get("k")
    disk.size()
    disk.get("k")
    disk.cache.wipe()
}

fun local(): Int {
    val declared: Cache = build()
    val created = Cache()
    val guessed = build()
    declared.clear()
    created.size(1)
    guessed.clear()
    guessed.get("k")
    return created.size()
}

fun String.loud(): String = this

fun extensions(cache: Cache, store: Store, disk: Disk, text: String) {
    cache.describe()
    cache.describe(true)
    store.tag()
    disk.tag()
    disk.describe()
    text.loud()
    text.shout()
    build().tag()
}

fun scopes(cache: Cache, disk: Disk, store: Store) {
    cache.apply {
        size()
        this.clear()
        build()
    }
    Cache().run { wipe() }
    with(disk) {
        flush()
        inherited()
    }
    cache.let { it.size(1) }
    disk.also { value -> value.flush() }
    store.also { build() }
    listOf(cache).forEach { it.wipe() }
    cache.apply { listOf(1).forEach { size() } }
}

// Opaque outside: a receiver typed by a class from outside the repository keeps its members
// unknown, and so does one with an outside base before a repository supertype.
class Mixed : Outside(), Store {
    override fun get(key: String): String = key

    override fun put(key: String, value: String) {}
}

fun outsiders(foreign: Outside, mixed: Mixed, list: List<Int>) {
    foreign.clear()
    mixed.put("a", "b")
    mixed.size()
    list.wipe()
}

// Extensions in scope as Kotlin sees them: the same package, explicit imports, star imports.
// One of a package the file does not import is out of scope (`kotlin.text.reversed` is called).
fun scoped(text: String) {
    text.reversed()
}

// A receiver of a type from outside the repository keeps its members unknown (Opaque outside):
// its extensions are name matches, and a call on the type's name reaches none of them.
fun Outside.spawn(): Int = 0

fun Outside.items() {}

fun Outside.orders() {
    items()
    this.items()
}

fun Map.Entry<String, Int>.entryLabel(): String = key

fun opaque(foreign: Outside, entries: Map<String, Int>) {
    foreign.items()
    Outside.spawn()
    entries.entryLabel()
}

// Extensions of `Any`, of a type parameter and of an outside supertype may apply to any
// receiver: name matches.
fun Any.toJson(): String = ""

fun <T> T.logged(): T = this

fun CharSequence.blankish(): Boolean = true

fun general(cache: Cache, text: String) {
    cache.toJson()
    text.toJson()
    text.logged()
    text.blankish()
}

// `X.f(..)` is a call, not a constructor: what it returns is unknown.
fun Outside.pretty(): String = ""

fun String.pretty(): String = this

fun called() {
    val shown = Outside.render(1)
    shown.pretty()
    val held = Outside.render(2)
    held.wipe()
}

// An untracked lambda's `it` is its own; the `let` around it keeps its typed one.
fun nestedIt(cache: Cache, disks: List<Disk>) {
    cache.let {
        it.size(1)
        disks.forEach { it.flush() }
    }
}

// A local function comes before the members of a scope function's receiver.
fun localFirst(cache: Cache) {
    fun size(): Int = 1
    cache.apply { size() }
}

// An unknown receiver and one extension of the name: a name match.
fun unknownReceiver() {
    build().loud()
}
