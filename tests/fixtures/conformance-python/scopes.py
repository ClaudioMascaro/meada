"""A class body's names are in scope for its own statements, not for its methods."""


def Helper():
    return 0


class Outer:
    class Helper:
        pass

    made = Helper()

    def m(self):
        return Helper()
