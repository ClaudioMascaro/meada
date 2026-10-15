"""Wrappers with parameter defaults: a call that leaves an argument out takes the default."""

import requests

from .config import HOST


def request(path, method="GET", base=HOST):
    return requests.request(method, base + path)


def list_defaults():
    # Method and base both left out.
    return request("/defaults")


def create_default():
    # The method by position, the base left out.
    return request("/defaults", "POST")


def other_base():
    # The base by keyword, the method left out.
    return request("/other", base="https://other.example.com")


def keyword_only(path, *, method="PUT"):
    return requests.request(method, HOST + path)


def call_keyword_only():
    return keyword_only("/kw-default")


def override_keyword_only():
    return keyword_only("/kw-override", method="PATCH")


def with_splats(path, *extra, method="HEAD", **options):
    return requests.request(method, HOST + path, **options)


def call_with_splats():
    # Positional arguments past `path` belong to `*extra`, not to `method`.
    return with_splats("/splat", "a", "b", timeout=3)
