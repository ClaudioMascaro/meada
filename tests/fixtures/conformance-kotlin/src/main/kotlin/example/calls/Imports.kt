package example.calls

import example.Point
import example.Registry as Lookup
import example.util.*
import example.util.tool as run
import example.legacy.Legacy

// Single imports of a class and an object (aliased), a star import, a function import with an
// alias, and a call into a Java class of the same repository.
fun imported(): Int {
    val origin = Point.origin()
    Lookup.find("x")
    run(1, 2)
    tool()
    internalTool()
    Tools.single()
    return Legacy.make() + origin.norm()
}

// Parameters and locals hide the names they bind.
fun shadowed(tool: () -> Int): Int {
    val run = { 0 }
    return tool() + run()
}

fun qualified(): Int = example.util.Tools.doubled() + example.helper()

fun unknown(): Int = outside() + Missing.call()
