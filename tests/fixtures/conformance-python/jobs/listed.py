"""Nothing calls these: a literal `__all__` is the public set, so `left_out` is no door."""

__all__ = ["listed"]


def listed():
    return 1


def left_out():
    return 2
