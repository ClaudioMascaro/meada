"""A name rebound in its scope is no longer the library's client or constructor."""

from confluent_kafka import Producer

producer = Producer({})


def rebound_by_parameter(producer):
    producer.produce("shadowed", b"{}")


def rebound_by_with(path):
    with open(path) as producer:
        producer.produce("shadowed")


def rebound_by_loop(producers):
    for producer in producers:
        producer.produce("shadowed")


def rebound_by_except():
    try:
        pass
    except Exception as producer:
        producer.produce("shadowed")


def rebound_constructor(Producer):
    client = Producer({})
    client.produce("shadowed", b"{}")
