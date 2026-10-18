"""A console script (`acme-report`) named in a package's `__init__.py`."""

from jobs.steps import fetch


def build():
    fetch()
