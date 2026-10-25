package example.legacy

// Extensions of a Java class and of `String`, which no Java call reaches.
fun Port.ext(s: String) {}

fun String.length(): Int = 0
