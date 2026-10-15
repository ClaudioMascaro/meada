from cycle_a import mine

other: "mine.Thing" = None


def use():
    return other.ring()
