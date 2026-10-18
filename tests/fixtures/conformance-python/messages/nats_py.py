"""nats-py: `publish` and `subscribe` on a connection."""

import nats
from nats import connect
from nats.aio.client import Client as NATS
from not_nats import connect as fake_connect

from .topics import REFUNDS

REGION = "eu"


async def bus():
    return await nats.connect("nats://nats:4222")


async def publish_all():
    # Literal, f-string and namespace-imported publishers, the subject by keyword too.
    nc = await connect("nats://nats:4222")
    other = await nats.connect("nats://nats:4222")
    await nc.publish("jobs.created", b"{}")
    await nc.publish(f"jobs.{REGION}.created", b"{}")
    await other.publish(subject="jobs.audit.created", payload=b"{}")


async def publish_legacy():
    nc = NATS()
    await nc.connect("nats://nats:4222")
    await nc.publish("jobs.legacy", b"{}")


async def publish_returned():
    # A connection a function returns.
    nc = await bus()
    await nc.publish(REFUNDS, b"{}")


async def announce(nc):
    # A connection a caller passes.
    await nc.publish("jobs.announced", b"{}")


async def announce_all():
    nc = await connect("nats://nats:4222")
    await announce(nc)


async def publish_unknown(subject):
    nc = await connect("nats://nats:4222")
    await nc.publish(f"{subject}.{len(subject)}", b"{}")


async def subscribe_one():
    # `*` is one token, `>` is the rest; each match is inferred.
    nc = await connect("nats://nats:4222")
    await nc.subscribe("jobs.*")


async def subscribe_rest():
    # The `cb` argument: a definition, and a lambda that is no definition (the function that
    # subscribes handles).
    nc = await connect("nats://nats:4222")
    await nc.subscribe("jobs.>", cb=on_job)
    await nc.subscribe("jobs.audit.>", cb=lambda message: record(message))


async def subscribe_middle():
    nc = await connect("nats://nats:4222")
    await nc.subscribe("jobs.*.created")
    await nc.subscribe("jobs.created")
    await nc.subscribe(REFUNDS)


async def not_published():
    # A connection of another package, and `publish` on any object.
    fake = await fake_connect()
    await fake.publish("jobs.created", b"{}")
    await Plain().publish("jobs.created")


class Plain:
    async def publish(self, subject):
        return subject


async def on_job(message):
    return record(message)


def record(value):
    return value
