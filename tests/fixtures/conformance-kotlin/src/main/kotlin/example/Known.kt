package example

import java.io.Serializable

// Known outside members: `Serializable` declares nothing and `Comparable` declares `compareTo`
// alone, so the class's own overloads are exact, `compareTo` stays a name match and a name
// nobody declares resolves to nothing.
class Ticket : Serializable, Comparable<Ticket> {
    fun open(): Int = 0

    fun open(seat: Int): Int = seat

    override fun compareTo(other: Ticket): Int = 0

    fun check(t: Ticket): Int = t.open(1) + open() + t.compareTo(this) + t.missing()
}
