package example.channels

import com.corp.shared.Foreign
import org.springframework.kafka.core.KafkaTemplate

class Pipe {
    fun send(topic: String, message: String) {}
}

class Holder(val kafka: Pipe)

class Plain(val other: Pipe)

// A member of an implicit receiver comes before the property of the class around: `kafka` is the
// holder's, whatever the receiver's type is, until a repository class has no such member.
class Receivers(private val kafka: KafkaTemplate<String, String>) {
    fun viaWith(holder: Holder) {
        with(holder) { kafka.send("orders.with", "m") }
    }

    fun viaRun(holder: Holder) {
        holder.run { kafka.send("orders.run", "m") }
    }

    fun viaApply(holder: Holder) {
        holder.apply { kafka.send("orders.apply", "m") }
    }

    fun viaOutside(foreign: Foreign) {
        with(foreign) { kafka.send("orders.outside", "m") }
    }

    fun viaPlain(plain: Plain) {
        with(plain) { kafka.send("orders.plain", "m") }
    }

    private fun Holder.viaExtension() {
        kafka.send("orders.extension", "m")
    }

    private fun Plain.viaPlainExtension() {
        kafka.send("orders.plain.extension", "m")
    }
}
