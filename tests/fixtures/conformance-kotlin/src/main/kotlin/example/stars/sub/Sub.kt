package example.stars.sub

import example.stars.*

// A base, an interface and a constructor named through a star import are the package's own.
class Sub : Base(), Face {
    fun go() {
        mark(2)
        runIt { mark(1) }
        face()
        Base().mark(3)
    }
}
