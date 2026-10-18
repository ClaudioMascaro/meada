"""Client wrappers: a function whose URL or method comes from its parameters."""

import httpx
import requests

from .config import HOST


def send(method, path):
    return requests.request(method, HOST + path)


# A second level of wrappers.
def get(path):
    return send("GET", path)


def remove(path):
    return send("DELETE", path)


def list_orders():
    return get("/orders")


def list_by_keyword():
    return get(path="/orders/kw")


def drop_order(order_id):
    return remove(f"/orders/{order_id}")


# A cycle: retry and backoff call each other before handing over to `get`.
def retry(path, attempts):
    if attempts > 0:
        return backoff(path, attempts)
    return get(path)


def backoff(path, attempts):
    return retry(path, attempts - 1)


def list_patiently():
    return retry("/orders/slow", 3)


class Api:
    """A wrapper that is a method, over a client with a base URL."""

    def __init__(self):
        self.http = httpx.Client(base_url=HOST + "/api")

    def user(self, user_id):
        return self.http.get(f"/users/{user_id}")

    def profile(self):
        return self.user("me")


class Inventory:
    """A method wrapper whose name is unique in the repository."""

    def __init__(self):
        self.session = requests.Session()

    def fetch_item(self, path):
        return self.session.get(HOST + path)


def stock():
    inventory = Inventory()
    return inventory.fetch_item("/stock")


def not_a_client(data):
    # Only the name matches `Inventory.fetch_item`: neither call is a consumer.
    data.fetch_item("/items")
    cfg = {}
    return cfg.fetch_item("/orders")


class Pages:
    """Methods called on the class: the instance passed explicitly, or a class method."""

    def __init__(self):
        self.http = httpx.Client(base_url=HOST)

    def load(self, path):
        return self.http.get(path)

    @classmethod
    def load_shared(cls, path):
        return requests.get(HOST + path)


def explicit_instance():
    pages = Pages()
    return Pages.load(pages, "/pages")


def class_method():
    return Pages.load_shared("/shared")


# A method whose name no other method of the repository has, called on a receiver of unknown
# class: the call matches by name only, so the client passed to it is no consumer.
class Reports:
    def pull(self, client):
        return client.get("/reports")


def unrelated(store):
    http = httpx.Client(base_url=HOST)
    return store.pull(http)
