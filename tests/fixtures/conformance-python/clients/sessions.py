"""Clients built from a constructor: sessions, and clients with a base URL."""

import aiohttp
import httpx
import requests

from .config import HOST

# A session in the module scope, used from functions.
session = requests.Session()


def from_module():
    return session.get(HOST + "/session")


def from_scope():
    s = requests.Session()
    return s.post(HOST + "/orders")


def with_base():
    client = httpx.Client(base_url=HOST + "/v2")
    # The base URL goes in front of the path, a trailing `/` of either side aside.
    client.get("/items")
    # An absolute URL ignores it.
    return client.options("https://other.example.com/probe")


def no_base():
    # Without a base URL the host stays unresolved.
    client = httpx.Client()
    return client.head("/ping")


def bound_by_with():
    with httpx.Client(base_url=HOST) as client:
        client.delete("/items/7")


async def bound_by_async_with():
    async with httpx.AsyncClient(base_url=HOST + "/") as client:
        await client.patch("items/9")
    async with aiohttp.ClientSession(base_url=HOST) as http:
        await http.request("HEAD", "/aio/head")
        async with http.get("/aio") as response:
            return response.get("etag")


class Cache:
    """What a `get` on is no HTTP call."""

    def get(self, key):
        return key


class Gateway:
    def __init__(self):
        self.http = httpx.Client(base_url=HOST + "/gateway")
        self.cache = Cache()

    def health(self):
        self.cache.get("health")
        return self.http.get("/health")


def annotated(client: httpx.Client):
    # A parameter is a client only when a caller passes one built from the constructor.
    return client.get("/annotated")


def passed(client):
    return client.put("/passed")


def hand_over():
    return passed(httpx.Client(base_url=HOST + "/handed"))
