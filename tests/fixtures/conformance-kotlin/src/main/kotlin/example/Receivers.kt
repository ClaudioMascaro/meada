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
