package example

import com.outside.Outside

// A bare call inside a class reads the class's family and its companion before the names
// around it: `log` and `origin` are also top-level functions of the package.
open class Base {
    fun log(message: String): Int = 0
}

fun log(message: String): Int = 1

fun origin(): Int = 2

class Derived : Base() {
    fun go(): Int = log("x")

    fun first(): Int = origin()

    companion object {
        fun origin(): Int = 0
    }
}

// Opaque outside: a base from outside the repository keeps its members unknown.
class Logged : Outside() {
    fun go(): Int = log("x")

    fun self(): Int = this.own()

    fun own(): Int = 0
}

// A companion's outside base lends nothing to the class's instances.
class Quiet {
    fun go(): Int = this.own()

    fun own(): Int = 0

    fun make(): Int = Quiet.build()

    companion object : Outside() {
        fun build(): Int = 0
    }
}

// A top-level function named like the package `example.util`: the package keeps its files.
fun util(): Int = 0

// Declarations in an initializer or a secondary constructor are no members.
class Init(val n: Int) {
    init {
        fun local(): Int = helper()
        val kept = local()
    }

    constructor() : this(0) {
        fun other(): Int = 0
    }
}

// Enum entry arguments are read in the enum's scope; an entry's body is opaque.
enum class Level(val weight: Int) {
    LOW(helper()),
    HIGH(2) {
        override fun toString(): String = label()
    };

    fun label(): String = "level"
}

// A class and its companion both declare `m()`: a call on the class's name reaches the
// companion's, a call on an instance (`this` too) the class's.
class Twin {
    fun m(): Int = 1

    fun own(): Int = this.m()

    companion object {
        fun m(): Int = 2
    }
}

fun twins(twin: Twin): Int = Twin.m() + twin.m()
