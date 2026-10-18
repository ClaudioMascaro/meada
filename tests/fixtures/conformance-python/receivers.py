"""Receivers the source binds again, which hide what an outer binding says of their class."""

try:
    from geometry import Circle
except ImportError:
    Circle = None


class Engine:
    def start(self):
        return 1


class Untyped:
    engine = Engine()

    def __init__(self, engine):
        self.engine = engine

    def run(self):
        return self.engine.start()


class Typed:
    engine: Engine

    def __init__(self, engine):
        self.engine = engine

    def run(self):
        return self.engine.start()


class Runner:
    def stride(self):
        return 2

    def each(self, items):
        for this in items:
            this.stride()


engine = Engine()
spare = Engine()


def module_read():
    return engine.start()


def with_as(open_engine):
    with open_engine() as engine:
        engine.start()


def comprehension(items):
    return [engine.start() for engine in items]


def walrus(items):
    if engine := items[0]:
        engine.start()


def except_as():
    try:
        pass
    except Exception as engine:
        engine.start()


def lambdas():
    return lambda engine: engine.start()


def param_reassigned(engine=None):
    engine.start()
    if engine is None:
        engine = Engine()


def swap():
    global spare
    spare = None


def spare_read():
    return spare.start()


def fallback():
    return Circle(1)


def lacking(engine: Engine):
    return engine.stride()


def optional(first: Engine | None, second: None | Engine):
    first.start()
    second.start()
