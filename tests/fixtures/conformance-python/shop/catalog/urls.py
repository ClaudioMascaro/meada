from django.urls import path
from django.views.decorators.http import require_GET, require_POST


@require_GET
def products(request):
    return []


@require_POST
def upload(request):
    return None


urlpatterns = [
    path("products/", products),
    path("upload/", upload),
]
