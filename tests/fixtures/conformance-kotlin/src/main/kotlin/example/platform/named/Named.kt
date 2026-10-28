package example.platform.named

import example.platform.Clock

// An `expect` class and its `actual` ones are one class, the `expect` one.
class Named : Clock() {
    fun go() {
        mark(2)
        tick()
        Clock()
    }
}
