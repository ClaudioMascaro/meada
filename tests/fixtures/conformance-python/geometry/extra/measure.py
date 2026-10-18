import geometry.shapes

from .. import units
from ..shapes import area_of


def perimeter(shape):
    return units.to_cm(area_of(shape)) + geometry.shapes.scale(2)
