package example.util

class Hidden {
    private fun closed(): Int = 0

    internal fun shared(): Int = closed()
}

private class FilePrivate
