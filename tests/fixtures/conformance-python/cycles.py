"""Names and bases that resolve through themselves end without a class."""


class Ping(Pong):
    def go(self):
        return self.ring()


class Pong(Ping):
    def ring(self):
        return 1


class Knot(Loop.Inner):
    def go(self):
        return self.ring()


class Loop(Knot.Inner):
    class Inner(Knot):
        pass


class Deep:
    x: "Deep.x.y"

    def go(self):
        return self.x.y.ring()


def crossed(a: "b.X", b: "a.Y"):
    a.ring()
    b.ring()
