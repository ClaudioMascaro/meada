from confluent_kafka import Consumer

from .main import audit


def apply_restock(message):
    return audit()


def consume_restocks():
    # A polling client: the function that subscribes to the topic handles it.
    consumer = Consumer({"bootstrap.servers": "kafka:9092", "group.id": "inventory"})
    consumer.subscribe(["stock.restocked"])
    while True:
        message = consumer.poll(1.0)
        if message is not None:
            apply_restock(message)
