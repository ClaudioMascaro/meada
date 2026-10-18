from .units import to_cm


def scale(value):
    return to_cm(value)


class Shape:
    def area(self):
        return 0

    @staticmethod
    def unit():
        return "cm"


class Circle(Shape):
    def __init__(self, radius):
        self.radius = radius

    def area(self):
        return scale(self.radius)

    async def describe(self):
        return to_cm(self.radius)


def area_of(shape):
    return shape.area()
