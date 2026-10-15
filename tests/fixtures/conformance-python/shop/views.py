"""Function views with no method restriction: they serve every method."""


def health(request):
    return "ok"


def vintage(request, code):
    return code


def archive(request, year):
    return year
