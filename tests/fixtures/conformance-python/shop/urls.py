"""The root URLconf: no other one includes it."""

from django.conf import settings
from django.conf.urls.static import static
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
    # A route with a part that cannot be read: no route.
    path(f"{settings.LEGAL_PREFIX}legal/", views.legal),
]

# Extensions join the list; an addition that is not a list literal is left out.
urlpatterns += [
    path("about/", views.about),
]
urlpatterns += static("/media/", document_root="media")
urlpatterns = urlpatterns + [path("contact/", views.contact)]
