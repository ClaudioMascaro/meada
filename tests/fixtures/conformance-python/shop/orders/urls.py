from django.urls import include, path

from . import views

urlpatterns = [
    path("orders/<int:pk>/", views.order),
    path("orders/", views.OrderList.as_view()),
    path("items/", include("shop.orders.items")),
]
