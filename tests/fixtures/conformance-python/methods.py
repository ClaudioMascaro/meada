"""Method calls: receivers whose class the source writes, and those only a name matches."""

from typing import Optional

from geometry import Circle
from geometry.shapes import Shape


class Base:
    def ping(self):
        return 1

    def shared(self):
        return 2


class Mixin:
    def shared(self):
        return 3

    def mixed(self):
        return 4


class Dial:
    def spin(self):
        return 5


class Panel(Base, Mixin):
    frame: Circle

    def __init__(self, shape: Shape):
        self.shape: Shape = shape

    def draw(self):
        self.ping()
        self.shared()
        self.mixed()
        self.render()
        self.shape.area()
        self.frame.describe()
        self.spin()

        def later():
            return self.render()

        return later()

    def render(self):
        return 0

    @classmethod
    def build(cls):
        return cls.create()

    @classmethod
    def create(cls):
        return cls(None)

    @staticmethod
    def check(value):
        return value.ping()


class Root:
    def hook(self):
        return 6


class Left(Root):
    pass


class Right(Root):
    def hook(self):
        return 7


class Leaf(Left, Right):
    def run(self):
        return self.hook()


def make():
    return Panel(None)


def use(panel: Panel, maybe: Optional[Panel], ahead: "Panel"):
    panel.draw()
    maybe.render()
    ahead.render()
    built = Panel(None)
    built.ping()
    made = make()
    made.render()
    made.area()


current = Panel(None)


def module_level():
    return current.render()


def shadowed(panels):
    current = panels[0]
    return current.render()
