import dataclasses
import functools
import os
from dataclasses import dataclass
from functools import lru_cache

import requests


class Client:
    def method_base(self):
        return os.environ["API_URL"]

    def ViaMethod(self):
        return requests.get(self.method_base() + "/api/orders")

    def method_prefix(self):
        return os.environ["API_URL"] + "/v2"

    def MethodWithPrefix(self):
        return requests.get(self.method_prefix() + "/orders")


def function_base():
    return os.getenv("API_URL") + "/v2"


def ViaFunction():
    return requests.get(function_base() + "/orders")


def plain_base():
    return os.getenv("API_URL")


def FunctionWithoutPrefix():
    return requests.get(plain_base() + "/api/orders")


def env_get(key, path):
    return requests.get(os.environ[key] + path)


def ParamA():
    return env_get("API_URL", "/api/orders")


def ParamB():
    return env_get("SECOND_URL", "/api/orders")


def ParamUnknown(key):
    return env_get(key, "/api/orders")


def getenv(key):
    return "https://different"


def shadowed_read(key):
    return requests.get(getenv(key) + "/api/orders")


def ShadowedEnvKey():
    return shadowed_read("API_URL")


def imported_read(key):
    return requests.get(os.getenv(key) + "/api/orders")


def ImportedEnvKey():
    return imported_read("API_URL")


def QueryAppend(flag):
    path = "/api/orders"
    if flag:
        path += "?x=1"
    return requests.get(os.getenv("API_URL") + path)


def QueryRewrite(flag):
    path = "/api/orders"
    if flag:
        path = path + "?x=1"
    return requests.get(os.getenv("API_URL") + path)


def QuerySuffix(flag):
    query = ""
    if flag:
        query = "?x=1"
    return requests.get(os.getenv("API_URL") + "/api/orders" + query)


def QueryConditional(flag):
    return requests.get(os.getenv("API_URL") + "/api/orders" + ("?x=1" if flag else ""))


def mutated(path):
    path = path + "/other"
    return requests.get(path)


def possibly_mutated(path, flag):
    if flag:
        path = "/v2/orders"
    return requests.get(path)


def intact(path):
    return requests.get(path)


def Reassigned():
    return mutated("https://service/api/orders")


def Conditional(flag):
    return possibly_mutated("https://service/api/orders", flag)


def Intact():
    return intact("https://service/api/orders")


# #949: a decorator call binds the name to what the factory returns, not to the body below.
def use_host(host):
    def decorate(original):
        def replacement():
            return host

        return replacement

    return decorate


@use_host("https://other.example")
def decorated_base():
    return os.getenv("API_URL")


def ViaDecoratedBase():
    return requests.get(decorated_base() + "/api/orders")


# #962: a class decorator may replace a method before the class is published.
def install(cls):
    def replacement(self):
        return "https://other.example"

    cls.changed_origin = replacement
    return cls


@install
class Changed:
    def changed_origin(self):
        return os.getenv("API_URL")

    def ViaChangedClass(self):
        return requests.get(self.changed_origin() + "/api/orders")


class Ordinary:
    def ordinary_origin(self):
        return os.getenv("API_URL")

    def ViaOrdinaryClass(self):
        return requests.get(self.ordinary_origin() + "/api/orders")


def register(name):
    def install_named(cls):
        cls.registered_origin = lambda self: "https://" + name + ".example"
        return cls

    return install_named


@register("other")
class Registered:
    def registered_origin(self):
        return os.getenv("API_URL")

    def ViaRegisteredClass(self):
        return requests.get(self.registered_origin() + "/api/orders")


# Decorators that keep the body a call runs.
@dataclass
class Settings:
    def settings_origin(self):
        return os.getenv("API_URL")

    def ViaDataclass(self):
        return requests.get(self.settings_origin() + "/api/orders")


@functools.lru_cache(maxsize=None)
def cached_base():
    return os.getenv("API_URL")


def ViaCachedBase():
    return requests.get(cached_base() + "/api/orders")


@lru_cache
def bare_cached_base():
    return os.getenv("API_URL")


def ViaBareCachedBase():
    return requests.get(bare_cached_base() + "/api/orders")


@dataclasses.dataclass
class ModuleSettings:
    def module_settings_origin(self):
        return os.getenv("API_URL")

    def ViaModuleDataclass(self):
        return requests.get(self.module_settings_origin() + "/api/orders")


# #962 through inheritance: a decorated heir may replace what its base defines.
def install_inherited(cls):
    cls.inherited_origin = lambda self: "https://other.example"
    return cls


class OriginBase:
    def inherited_origin(self):
        return os.getenv("API_URL")


@install_inherited
class InheritingChild(OriginBase):
    def ViaInheritedClass(self):
        return requests.get(self.inherited_origin() + "/api/orders")


def ViaChildInstance():
    return requests.get(InheritingChild().inherited_origin() + "/api/orders")


# A decorated ancestor does not rebind what a plain heir defines.
@register("base")
class DecoratedBase:
    pass


class PlainChild(DecoratedBase):
    def child_origin(self):
        return os.getenv("API_URL")

    def ViaPlainChild(self):
        return requests.get(self.child_origin() + "/api/orders")
