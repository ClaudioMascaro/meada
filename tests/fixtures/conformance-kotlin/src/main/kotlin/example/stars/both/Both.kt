package example.stars.both

import example.stars.*
import example.stars.other.Base

// An explicit import wins over a star import.
class Both : Base() {
    fun go() {
        mark(2)
        Base()
    }
}
