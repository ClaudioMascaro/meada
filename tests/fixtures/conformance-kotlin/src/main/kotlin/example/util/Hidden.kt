package example.util

class Hidden {
    private fun closed(): Int = 0

    internal fun shared(): Int = closed()

    fun visible(): Int = 0
}

// The package's other files are visible without an import.
fun viaPackage(): Int = tool()

private class FilePrivate
