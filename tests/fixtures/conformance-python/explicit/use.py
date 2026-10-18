"""Explicit imports and attribute access see every name; only `import *` is limited."""

import explicit.mod as m
import shadow
from explicit import beta, default
from explicit.first import pick
from explicit.mod import _hidden, outside
from explicit.second import pick
from explicit.util import *
from requests import get
from shadow import from_package


def report():
    outside()
    _hidden()
    m.outside()
    m._hidden()
    beta()
    default()
    # The later import binds the name.
    pick()
    # An explicit import of a package outside the repository wins over a star import.
    get()
    from_package()
    shadow.from_package()
