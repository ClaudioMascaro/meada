import httpx
import requests

from settings import BASE, load_base

client = httpx.Client(base_url=BASE)


def via_client():
    return client.get("/orders")


def via_call():
    return requests.get(load_base() + "/items")
