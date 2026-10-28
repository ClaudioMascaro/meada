package ex

class Holder {
    fun m(a: Int) {}
}

fun m(a: Int) {}

fun run(f: (Any) -> Unit) {
    f { x -> x.m(1) }
}
