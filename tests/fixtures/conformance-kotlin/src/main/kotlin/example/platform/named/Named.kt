package example.platform.named

import example.platform.Clock

// An `expect` class and its `actual` ones are one class, the `expect` one; a member only an
// `actual` declares is a name match.
class Named : Clock() {
    fun go() {
        mark(2)
        tick()
        Clock()
    }

    fun hold(clock: Clock) {
        clock.mark(1)
        clock.tick()
    }
}
