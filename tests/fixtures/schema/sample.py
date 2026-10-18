"""A sample of the facts the Python pack stores, for the schema golden."""

import os.path
from shapes import Shape
from . import helpers
from .models import *

__all__ = ["Circle", "area"]

BASE = "/api"


class Circle(Shape):
    radius: float

    def __init__(self, radius: float):
        self.radius = radius
        self.client = Client()

    @classmethod
    def unit(cls, *args, scale=1, **kwargs):
        return cls(1)

    @staticmethod
    def describe(*, label):
        return label


@app.get("/circles/{id}")
@require_GET
def area(circle: Circle, factor=2):
    with open(os.path.join(BASE, "x")) as handle:
        handle.read()
    return circle.radius * factor


def main():
    shape = Circle(3)
    helpers.log(area(shape))


from .extras import *
