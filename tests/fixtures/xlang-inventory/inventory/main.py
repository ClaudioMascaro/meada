import os
from os import environ, getenv

import httpx
import requests
from fastapi import FastAPI

app = FastAPI()

STOREFRONT = os.environ["STOREFRONT_URL"]


@app.get("/stock/{sku}")
def stock_level(sku: str):
    # Through a module name that holds the environment read.
    return requests.get(f"{STOREFRONT}/api/prices/{sku}")


@app.post("/stock/reserve")
def reserve():
    # A fallback and a string method, on the client's base URL.
    client = httpx.Client(base_url=(getenv("STOREFRONT_URL") or "http://localhost:3000").rstrip("/"))
    return client.get("/api/promotions")


def audit():
    # A default argument and a string method.
    return requests.post(environ.get("STOREFRONT_URL", "http://localhost:3000").strip() + "/api/audit")
