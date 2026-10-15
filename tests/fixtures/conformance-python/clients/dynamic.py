"""Calls whose path cannot be evaluated: an unresolved consumer, not a missing one."""

import os

import requests

from .config import HOST


def formatted(order_id):
    # `str.format` is not folded.
    return requests.get("{}/orders/{}".format(HOST, order_id))


def percent(order_id):
    # Nor is `%`.
    return requests.delete("%s/orders/%d" % (HOST, order_id))


def computed():
    return requests.post(build_url())


def build_url():
    return HOST + "/built"


def from_environment():
    # An environment read is the base: the path is still read (see `environment.py`).
    return requests.get(os.environ["ORDERS_URL"] + "/orders")


def unknown_method(method):
    # Nobody calls it: the method stays unknown.
    return requests.request(method, HOST + "/orders")
