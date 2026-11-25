from .tracking import track


@track()
def handler():
    return 1
