package example

import example.util.Tools as T

class Store(val name: String = "store") {
    fun save(id: Int, vararg tags: String) = T.check(id)

    companion object {
        fun open(): Store = Store()
    }
}

internal fun helper(): Store = Store.open()

expect class Clock

actual class Timer

class OnlySecondary {
    constructor(a: Int) {}
}
