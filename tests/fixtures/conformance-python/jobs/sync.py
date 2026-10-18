"""A console script (`acme-sync`) in a file that also has a main guard."""

from jobs.steps import fetch, store


def run():
    fetch()
    store()


if __name__ == "__main__":
    run()
    store()
