"""The root URLconf: no other one includes it."""

from django.urls import include, path, re_path

from . import views
from .catalog import urls as catalog_urls

urlpatterns = [
    path("health/", views.health),
    # By dotted name, two levels deep.
    path("shop/", include("shop.orders.urls")),
    # By an imported module.
    path("catalog/", include(catalog_urls)),
    re_path(r"^vintage/(?P<code>\d+)/$", views.vintage),
    # An unnamed group and a quantifier: no route.
    re_path(r"^archive/(\d{4})/$", views.archive),
]
