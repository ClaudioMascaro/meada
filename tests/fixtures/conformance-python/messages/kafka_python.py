"""kafka-python: `KafkaProducer.send`, the topics of `KafkaConsumer(...)` and `subscribe`."""

import kafka
from kafka import KafkaConsumer, KafkaProducer
from kafka import KafkaConsumer as KC
from not_kafka import KafkaProducer as FakeProducer

producer = KafkaProducer(bootstrap_servers="kafka:9092")
fake = FakeProducer()


def send_invoice():
    producer.send("invoices", b"{}")


def send_retry():
    producer.send("retries", b"{}")


class Relay:
    def __init__(self):
        self.producer = kafka.KafkaProducer(bootstrap_servers="kafka:9092")

    def forward(self, payload):
        # An attribute assigned the constructor once in its class, the topic by keyword.
        self.producer.send(topic="audits", value=payload)


def listen_invoices():
    # The topics are the constructor's positional arguments; the function that builds it handles.
    consumer = KafkaConsumer("invoices", "audits", bootstrap_servers="kafka:9092")
    for message in consumer:
        print(message)


def listen_retries():
    consumer = kafka.KafkaConsumer(bootstrap_servers="kafka:9092")
    consumer.subscribe(topics=["retries"])
    return consumer


def listen_alias():
    # A constructor imported under another name.
    return KC("alias", bootstrap_servers="kafka:9092")


def idle():
    # A constructor with no topic subscribes to nothing.
    return KafkaConsumer(bootstrap_servers="kafka:9092")


def not_sent():
    fake.send("invoices")
