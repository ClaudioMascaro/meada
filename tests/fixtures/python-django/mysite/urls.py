"""The root URLconf. The vendor app lives under `apps/`, which is no source root, so its
dotted name does not resolve."""

from django.urls import include, path

from . import views

extra_patterns = [
    path("ping/", views.ping),
]

urlpatterns = [
    path("health/", views.health),
    # The tuple form: a module or a list of entries, then the application namespace.
    path("shop/", include(("shop.urls", "shop"), namespace="shop")),
    path("extra/", include((extra_patterns, "extra"))),
    # A dotted name Meada cannot resolve: the URLconf it may name is served nowhere.
    path("vendor/", include("vendor.urls")),
]
