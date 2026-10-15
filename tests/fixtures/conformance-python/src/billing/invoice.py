from . import tax
from .tax import vat


def total(amount):
    return amount + vat(amount) + tax.fee(amount)
