from . import shared_lib
from . import sl_extras as extras
from .shared_lib import greet
from ..shared_lib.api import fetch


def relative(name):
    shared_lib.greet(name)
    extras.boost(name)
    fetch(name)
    return greet(name)
