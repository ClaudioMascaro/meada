"""pika: `basic_publish`, `basic_consume` and `queue_bind` on a channel of a connection."""

import os

import pika
from not_pika import BlockingConnection as FakeConnection
from pika import BlockingConnection

from .topics import EVENTS

connection = pika.BlockingConnection(pika.ConnectionParameters("rabbit"))
channel = connection.channel()
fake_connection = FakeConnection()
fake = fake_connection.channel()


def enqueue():
    # The default exchange is the queue its routing key names.
    channel.basic_publish(exchange="", routing_key="tasks", body=b"{}")
    channel.basic_publish("", "invoices", b"{}")


# A queue wrapper: each call site of `queue` produces to its own queue.
def queue(name):
    return channel.basic_publish(exchange="", routing_key=name, body=b"{}")


def enqueue_reports():
    return queue("reports")


def enqueue_unknown():
    channel.basic_publish(exchange="", routing_key=os.environ["QUEUE"], body=b"{}")


def announce():
    # Publishes to the exchange `events`: they reach the queues bound to it.
    channel.basic_publish(exchange=EVENTS, routing_key="order.created", body=b"{}")
    channel.basic_publish(EVENTS, "order.shipped", b"{}")
    channel.basic_publish(exchange=EVENTS, routing_key="refund.paid", body=b"{}")


def bind():
    # A pattern with no wildcard, `#` (zero or more words) and `*` (one word).
    channel.queue_bind(queue="audit", exchange=EVENTS, routing_key="order.created")
    channel.queue_bind(queue="alerts", exchange=EVENTS, routing_key="order.#")
    channel.queue_bind("shipments", EVENTS, "*.shipped")


def listen():
    # The callback by keyword and by position; a lambda is no definition, so the function that
    # consumes handles.
    channel.basic_consume(queue="audit", on_message_callback=on_audit, auto_ack=True)
    channel.basic_consume("alerts", on_alert)
    channel.basic_consume(queue="shipments", on_message_callback=lambda ch, method, properties, body: record(body))
    channel.basic_consume(queue="tasks", on_message_callback=on_task)
    channel.basic_consume(queue="mail", on_message_callback=on_mail)


def on_audit(ch, method, properties, body):
    return record(body)


def on_alert(ch, method, properties, body):
    return record(body)


def on_task(ch, method, properties, body):
    return record(body)


def on_mail(ch, method, properties, body):
    return record(body)


def record(value):
    return value


def send_mail():
    # A connection bound by `with`, and its channel.
    with pika.BlockingConnection(pika.ConnectionParameters("rabbit")) as bound:
        sender = bound.channel()
        sender.basic_publish("", "mail", b"{}")


def send_chained():
    # Clients written as a chain of calls: a channel made in place, and a method called on a
    # channel made in place.
    sender = pika.BlockingConnection(pika.ConnectionParameters("rabbit")).channel()
    sender.basic_publish("", "chained", b"{}")
    pika.BlockingConnection(pika.ConnectionParameters("rabbit")).channel().basic_publish(
        "", "inline", b"{}"
    )


class Worker:
    def __init__(self):
        self.connection = BlockingConnection(pika.ConnectionParameters("rabbit"))
        self.channel = self.connection.channel()

    def consume(self):
        # A channel kept as an attribute, and a method as the callback.
        self.channel.basic_consume(queue="reports", on_message_callback=self.on_report)

    def on_report(self, ch, method, properties, body):
        return record(body)

    def enqueue(self):
        self.channel.basic_publish("", "reports", b"{}")


def not_published():
    # A channel of another package.
    fake.basic_publish("", "tasks", b"{}")
    fake.basic_consume("tasks", on_task)
