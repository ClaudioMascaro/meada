"""The module functions of requests and httpx, and what `get` can be besides them."""

import httpx
import requests
from requests import get as fetch_page

from . import config
from .config import HOST, USERS


def list_users():
    # A string literal and an imported constant joined with `+`.
    return requests.get(HOST + USERS)


def create_user(payload):
    # An f-string holding an imported constant.
    return httpx.post(f"{HOST}/users", json=payload)


def rename_user(user_id):
    # An f-string hole that does not fold becomes a parameter marker.
    return requests.put(f"{HOST}/users/{user_id}/name")


def status():
    # A function imported from the library under another name.
    return fetch_page(HOST + "/status", timeout=3)


def purge():
    # `request(method, url)`, with keyword arguments.
    requests.request("DELETE", HOST + "/cache")
    return httpx.request(method="PATCH", url=HOST + "/cache/flags")


def lookup(options, cache):
    # `get` on any other object is not a consumer.
    cache.get("key")
    return options.get("retries", 3)


def list_orders():
    # A module's attribute joined with `+`.
    return requests.get(config.HOST + "/orders")


def read_order(order_id):
    # A module's attribute inside an f-string.
    return requests.get(f"{config.HOST}/orders/{order_id}")
