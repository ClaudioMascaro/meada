"""Every definition form."""

import functools


def plain(value):
    return value


async def fetch_all(items):
    return [plain(item) for item in items]


def outer():
    def inner():
        return plain(1)

    return inner()


def register(func):
    @functools.wraps(func)
    def wrapper(*args, **kwargs):
        return func(*args, **kwargs)

    return wrapper


@register
def decorated():
    return plain(2)


@register
class Registry:
    items = [plain(3)]

    def add(self, item):
        return self.items.append(item)

    async def load(self):
        return await fetch_all(self.items)

    @staticmethod
    def unit():
        return plain(4)

    @classmethod
    def build(cls):
        return cls()

    @register
    def tagged(self):
        return plain(5)

    class Meta:
        def label(self):
            return plain(6)


key = lambda entry: plain(entry)


def sorted_names(names):
    return sorted(names, key=lambda name: plain(name))
