import json

import shared_lib
import sl_extras
from shared_lib import greet
from shared_lib.api import Client


def welcome(name):
    return greet(name)


def lookup(key):
    return shared_lib.api.fetch(key)


def make():
    return Client.create()


def extra(value):
    return sl_extras.boost(value)


def hidden(name):
    greet = str
    return greet(name)


def encode(value):
    return json.dumps(value)
