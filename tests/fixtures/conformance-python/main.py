"""Entry points: every import form and call form."""

import geometry
import geometry.extra.measure
import geometry.units as gu
import billing.tax as tax
import numpy as np
from billing import total
from geometry import Circle as Round, area_of
from geometry.shapes import scale as grow
from helpers import *
from stars.computed import *
from stars.literal import *
from stars.underscore import *


def main():
    circle = Round(2)
    area_of(circle)
    geometry.area_of(circle)
    geometry.units.to_cm(1)
    geometry.extra.measure.perimeter(circle)
    gu.to_cm(1)
    grow(3)
    total(10)
    tax.vat(10)
    format_name("x")
    clamp(1, 2)
    listed()
    Shown()
    unlisted()
    visible()
    _internal()
    named()
    unnamed()
    np.array([1])
    legacy()
    circle.describe()
