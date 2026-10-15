"""Class families: Python's method resolution order, and bases the repository does not define."""

import httpx
from ext import ExtBase


class F:
    pass


class E:
    def pick(self):
        return "E"


class D:
    def pick(self):
        return "D"


class C(D, F):
    pass


class B(D, E):
    pass


class A(B, C):
    def go(self):
        return self.pick()


class Tangled(D, B):
    def go(self):
        return self.pick()


class Stored:
    def save(self):
        return 3


class Model(ExtBase, Stored):
    def go(self):
        return self.save()


class Client(httpx.Client):
    pass


class Remote(Client, Stored):
    def go(self):
        return self.save()


class Plain(object):
    def keep(self):
        return self.go()

    def go(self):
        return 4


class Lone:
    def go(self):
        return self.pick()

    @classmethod
    def make(cls):
        return cls.pick()
