"""A URLconf included from an included one."""

from django.urls import path
from django.views.decorators import http


@http.require_safe
def stock(request, sku):
    return sku


urlpatterns = [
    path("<sku>/stock", stock),
]
