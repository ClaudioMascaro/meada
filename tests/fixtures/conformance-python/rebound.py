"""The last top-level binding of a name decides: one after a definition, or a `del`, hides it."""


def replaced():
    return 1


replaced = 2


def deleted():
    return 3


del deleted


def grown():
    return 4


from stars.grown import *  # noqa: E402


def reach():
    replaced()
    deleted()
    grown()
