package example.other

// Locals and a parameter named like the properties of `example.jvm.Injected`: assigning them
// writes no property.
fun shadow() {
    var configured = "a"
    configured = "b"
}

fun param(literal: String) {
    var again = literal
    again = "c"
}
