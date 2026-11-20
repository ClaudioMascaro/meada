import os
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
