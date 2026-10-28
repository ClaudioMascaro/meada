package example.stars.two

import example.stars.*
import example.stars.other.*

// Two star imports that give one name leave it ambiguous.
class Two : Base() {
    fun go() {
        mark(2)
        Base()
    }
}
