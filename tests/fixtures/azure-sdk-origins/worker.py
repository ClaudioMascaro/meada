from azure.servicebus import ServiceBusClient as Bus, ServiceBusSender
from azure.eventhub import EventHubProducerClient as Producer
from azure.storage.blob import BlobServiceClient as Storage
from azure.keyvault.secrets import SecretClient as Secrets
bus = Bus(namespace, credential)
def sender(name):
    return bus.get_queue_sender(name)
def receiver(topic, sub):
    return bus.get_subscription_receiver(topic, sub)
def run():
    sender('factory-first-python').send_messages(message)
    sender('factory-second-python').send_messages(message)
    receiver('topic-first-python', 'audit').receive_messages(1)
    receiver('topic-second-python', 'ops').receive_messages(1)
def hidden(Bus, Producer, Storage, Secrets):
    Bus(namespace, credential).get_queue_sender('hidden-bus-python').send_messages(message)
    Producer(namespace, 'hidden-hub-python', credential).send_batch(batch)
    Storage(endpoint, credential).get_blob_client('hidden-container-python', 'object').upload_blob(data)
    Secrets(vault, credential).get_secret('hidden-secret-python')
def typed(client: ServiceBusSender):
    client.send_messages(message)
def reassigned():
    client = bus.get_queue_sender('stale-python')
    client = opaque
    client.send_messages(message)
def alternative(flag):
    if flag:
        return bus.get_queue_sender('unsafe-python')
    return opaque
def opaque_path():
    alternative(flag).send_messages(message)
def name_matched():
    foreign.sender('matched-python').send_messages(message)
def unsupported():
    bus.get_queue_sender('unsupported-python').invented_send(message)
def handle_only():
    return bus.get_queue_sender('handle-only-python')
def unaffected():
    bus.get_queue_sender('unaffected-python').send_messages(message)

class Factory:
    def make_client(self):
        return Bus(namespace, credential)
def guess_name(unknown):
    unknown.make_client().get_queue_sender('matched-constructor').send_messages(message)
