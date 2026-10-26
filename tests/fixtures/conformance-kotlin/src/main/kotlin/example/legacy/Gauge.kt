package example.legacy

// An extension written in its class's file is no member of the class, and a member of a Java
// base comes before an extension of the name.
class Gauge {
    fun read(): Int = 0
}

fun Gauge.reset() {}

class Meter : Port()

fun Meter.ext(x: Int) {}

fun gauges(gauge: Gauge, meter: Meter) {
    gauge.reset()
    meter.ext(1)
}
