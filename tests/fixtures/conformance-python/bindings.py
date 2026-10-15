"""Bindings that hide an outer one wherever they write the name, and shared outside bases."""

from abc import ABC


class Line:
    def dial(self):
        return 1


class Unpacked:
    line = Line()

    def __init__(self, make):
        self.line, self.other = make(), 1

    def call(self):
        return self.line.dial()


class Looped:
    line = Line()

    def __init__(self, items):
        for self.line in items:
            pass

    def call(self):
        return self.line.dial()


class Opened:
    line = Line()

    def __init__(self, open_line):
        with open_line() as self.line:
            pass

    def call(self):
        return self.line.dial()


class Reset:
    line = Line()

    def call(self, make):
        def reset():
            self.line = make()

        reset()
        return self.line.dial()


class Outside:
    line = Line()

    def call(self):
        return self.line.dial()


def poke(outside: Outside, make):
    outside.line = make()


def nested(make):
    line = Line()

    def mid():
        def inner():
            nonlocal line
            line = make()

        inner()

    mid()
    return line.dial()


def factory(make):
    line = Line()

    class Inner:
        line = Line()

        def reset(self):
            nonlocal line
            line = make()

    Inner().reset()
    return line.dial()


def matched(value):
    line = Line()
    match value:
        case [line]:
            pass
    return line.dial()


def imported():
    line = Line()
    import os as line

    return line.dial()


def lazy():
    from geometry.shapes import scale

    return scale(1)


class Left(ABC):
    def left(self):
        return 1


class Right(ABC):
    def right(self):
        return 2


class Both(Left, Right):
    def go(self):
        return self.right()


class ObjectLeft(object):
    pass


class ObjectRight(object):
    def side(self):
        return 3


class ObjectBoth(ObjectLeft, ObjectRight):
    def go(self):
        return self.side()


def mixin():
    return object


class Store:
    def stash(self):
        return 4


class Mixed(mixin(), Store):
    def go(self):
        return self.stash()
