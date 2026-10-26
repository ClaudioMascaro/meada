package example.legacy;

// Java reaches no Kotlin extension, not even one named under its class: `meter.ext(1)` is the
// member of the Java base.
public class Gauges {
    void use(Gauge gauge, Meter meter) {
        gauge.reset();
        meter.ext(1);
    }
}
