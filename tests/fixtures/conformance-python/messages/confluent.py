"""confluent-kafka: `Producer.produce` and `Consumer.subscribe`."""

import os

import confluent_kafka
from confluent_kafka import Consumer, Producer
from not_kafka import Producer as FakeProducer

from .topics import ORDERS, REFUNDS

producer = Producer({"bootstrap.servers": "kafka:9092"})
other = confluent_kafka.Producer({"bootstrap.servers": "kafka:9092"})
fake = FakeProducer({})


def produce_all():
    # The topic by position, by keyword, and built from an imported constant.
    producer.produce(ORDERS, b"{}")
    producer.produce(topic="payments", value=b"{}")
    other.produce(f"{REFUNDS}.eu", b"{}")


# A client wrapper: each call site of `emit` produces, with its topic.
def emit(topic):
    return producer.produce(topic, b"{}")


def emit_refund():
    return emit(REFUNDS)


def consume_orders():
    # A polling client: the function that subscribes handles.
    consumer = Consumer({"group.id": "orders"})
    consumer.subscribe([ORDERS, "payments"])
    return consumer.poll(1.0)


def consume_refunds():
    consumer = Consumer({"group.id": "refunds"})
    consumer.subscribe(topics=[REFUNDS, f"{REFUNDS}.eu"])
    return consumer.poll(1.0)


def produce_unknown():
    # A topic read from the environment cannot be evaluated: an unresolved producer.
    producer.produce(os.environ["TOPIC"], b"{}")


class Bus:
    def relay(self, topic):
        return producer.produce(topic, b"{}")


def relay_dynamic(bus):
    # `bus` may be any object: the call that reaches `relay` by its name is not climbed, so
    # the topic of `relay` stays unknown.
    bus.relay("never")


class Counter:
    def produce(self, topic):
        return topic


def not_produced():
    # A producer of another package, and `produce` on an object of the repository.
    counter = Counter()
    fake.produce(ORDERS)
    counter.produce(ORDERS)
