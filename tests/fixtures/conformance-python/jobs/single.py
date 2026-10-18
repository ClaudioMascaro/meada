"""A main guard written the other way round, with single quotes."""

from jobs import steps

if '__main__' == __name__:
    steps.fetch()
else:
    steps.store()
