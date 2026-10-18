"""redis-py: `xadd` and the streams of `xreadgroup`."""

import os

import redis
import redis.asyncio as aioredis
from not_redis import Redis as FakeRedis
from redis import Redis

from .topics import ORDERS, SHIPMENTS

client = redis.Redis(host="redis")
pool = aioredis.Redis(host="redis")
imported = Redis.from_url("redis://redis")
fake = FakeRedis()


def append_all():
    # The stream by position, by keyword and from an f-string over a constant.
    client.xadd(ORDERS, {"id": "1"})
    client.xadd(name=SHIPMENTS, fields={"id": "1"})
    pool.xadd(f"{SHIPMENTS}.eu", {"id": "1"})
    imported.xadd("returns", {"id": "1"})


# A client wrapper: each call site of `append` produces, with its stream.
def append(stream):
    return client.xadd(stream, {"id": "1"})


def append_audit():
    return append("events.audit")


def append_unknown():
    return client.xadd(os.environ["STREAM"], {"id": "1"})


def read_orders():
    # The consumer group's streams are the keys of the dictionary.
    return client.xreadgroup("workers", "w1", {"orders": ">"}, count=10)


def read_many():
    # The streams by keyword, one named by a constant and one by an f-string.
    return pool.xreadgroup(
        groupname="workers",
        consumername="w1",
        streams={ORDERS: ">", f"{SHIPMENTS}.eu": ">"},
        block=1000,
    )


def read_plain():
    # Without a consumer group, a read is no consumer.
    return client.xread({"orders": "$"})


def read_unknown():
    return client.xreadgroup("workers", "w1", {os.environ["STREAM"]: ">"})


def not_streamed():
    # A client of another package.
    fake.xadd("orders", {"id": "1"})
    fake.xreadgroup("workers", "w1", {"orders": ">"})
