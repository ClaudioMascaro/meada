import os
from azure.servicebus import ServiceBusClient
bus = ServiceBusClient(namespace, credential)
def receive():
    bus.get_subscription_receiver(os.getenv('TOPIC'), os.getenv('SUB')).receive_messages(1)
def queue():
    bus.get_queue_receiver(os.getenv('QUEUE')).receive_messages(1)
def literal_topic():
    bus.get_subscription_receiver('literal', os.getenv('SUB')).receive_messages(1)
def literal_sub():
    bus.get_subscription_receiver(os.getenv('TOPIC'), 'fixed').receive_messages(1)
def unread():
    bus.get_subscription_receiver(os.getenv('TOPIC'), os.getenv('UNREAD')).receive_messages(1)

def literal_queue():
    bus.get_queue_receiver('unaffected-python').receive_messages(1)
