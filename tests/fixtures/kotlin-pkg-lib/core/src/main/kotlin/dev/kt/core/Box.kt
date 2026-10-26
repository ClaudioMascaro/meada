package dev.kt.core

class Box(val n: Int) {
    fun of(i: Int): Box = Box(i)

    companion object {
        fun of(i: Int): Box = Box(i)
    }
}
