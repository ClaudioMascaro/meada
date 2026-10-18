"""Nothing calls these: only the public name is an export entry point."""


def public():
    return 1


def _helper():
    return 2
