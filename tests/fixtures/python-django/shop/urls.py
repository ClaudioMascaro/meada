from django.urls import path

from . import views

app_name = "shop"
urlpatterns = [
    path("items/", views.items),
]
