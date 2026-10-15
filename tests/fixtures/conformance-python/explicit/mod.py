"""`__all__` and the underscore prefix limit `import *`, not an explicit import."""

__all__ = ["public"]


def public():
    return 1


def outside():
    return 2


def _hidden():
    return 3
