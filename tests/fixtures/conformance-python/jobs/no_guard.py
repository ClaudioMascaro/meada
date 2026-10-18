"""Conditions that are no main guard: their calls stay at the top level of the file."""

from jobs.steps import fetch, store

if __name__ != "__main__":
    fetch()

if __name__ == "other" or __name__ == "__main__":
    store()
