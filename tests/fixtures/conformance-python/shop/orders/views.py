from django.views import View
from django.views.decorators.http import require_http_methods


@require_http_methods(["GET", "DELETE"])
def order(request, pk):
    return str(pk)


class OrderList(View):
    """A class-based view serves the methods it defines."""

    def get(self, request):
        return []

    def post(self, request):
        return self.total()

    def total(self):
        return 0
