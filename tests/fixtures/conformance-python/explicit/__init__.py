"""A package whose `__all__` lists one of the names it imports, and which star-imports a
module with a function named `default`."""

__all__ = ["alpha"]

from .core import *
from .letters import alpha, beta
