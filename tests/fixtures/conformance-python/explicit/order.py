"""The latest import that binds a name decides, whatever it resolves to."""

from explicit.early import kept, swept, taken
from explicit.sweeper import *
from requests import taken


def order():
    taken()
    swept()
    kept()
