import os

import requests


def refresh_prices(sku):
    # `os.getenv` with a default argument.
    return requests.get(os.getenv("STOREFRONT_URL", "http://localhost:3000") + "/api/prices/" + sku)


def warm():
    # Through the storefront's declared host.
    return requests.get("https://storefront.example.com/api/health")
