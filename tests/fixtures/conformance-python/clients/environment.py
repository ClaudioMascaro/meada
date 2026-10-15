"""Base URLs read from the environment: Meada names the variable and never reads its value."""

import os
from os import environ, getenv

import httpx
import requests

BASE = os.environ["ORDERS_URL"]


def subscript():
    return requests.get(os.environ["ORDERS_URL"] + "/orders")


def imported_subscript():
    return requests.get(environ["ORDERS_URL"] + "/orders")


def environ_get():
    return requests.get(os.environ.get("ORDERS_URL") + "/orders")


def getenv_call():
    return requests.get(os.getenv("ORDERS_URL") + "/orders")


def imported_getenv():
    return requests.get(getenv("ORDERS_URL") + "/orders")


def fallback():
    return requests.get((os.getenv("ORDERS_URL") or "http://localhost:8000") + "/orders")


def default_argument():
    return requests.get(os.getenv("ORDERS_URL", "http://localhost:8000") + "/orders")


def default_in_environ_get():
    return requests.get(environ.get("ORDERS_URL", "http://localhost:8000") + "/orders")


def string_methods():
    return requests.get(os.environ["ORDERS_URL"].strip().rstrip("/") + "/orders")


def named():
    return requests.get(f"{BASE}/orders")


def interpolated():
    return requests.get(f"{os.environ['ORDERS_URL']}/orders")


def client_base_url():
    client = httpx.Client(base_url=getenv("ORDERS_URL", "http://localhost:8000").rstrip("/"))
    return client.get("/orders")


def two_variables():
    # Two variables are a hole: no base is named.
    return requests.get((os.getenv("ORDERS_URL") or os.getenv("API_URL")) + "/orders")


def default_reads_another():
    return requests.get(os.getenv("ORDERS_URL", os.getenv("API_URL")) + "/orders")


def computed_name(name):
    # A variable named at run time is not an environment read Meada can name.
    return requests.get(os.environ[name] + "/orders")
