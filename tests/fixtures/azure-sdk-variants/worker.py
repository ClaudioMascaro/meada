from azure.servicebus import ServiceBusClient as Bus
from azure.eventhub import EventHubProducerClient as Producer, EventHubConsumerClient as Consumer
from azure.storage.blob import BlobServiceClient as Storage
from azure.keyvault.secrets import SecretClient as Secrets
bus = Bus(fully_qualified_namespace=namespace, credential=credential)
sender = bus.get_queue_sender(queue_name='orders-python')
receiver = bus.get_queue_receiver(queue_name='orders-python')
subscription = bus.get_subscription_receiver(topic_name='orders-python', subscription_name='audit')
producer = Producer(fully_qualified_namespace=namespace, eventhub_name='events-python', credential=credential)
consumer = Consumer(fully_qualified_namespace=namespace, eventhub_name='events-python', consumer_group='group', credential=credential)
storage = Storage(account_url=endpoint, credential=credential)
blob = storage.get_blob_client(container='uploads-python', blob=unknown_object)
secrets = Secrets(vault_url=vault, credential=credential)
def send():
    sender.send_messages(message)
def receive():
    receiver.receive_messages(max_message_count=1)
def subscribe():
    subscription.receive_messages(max_message_count=1)
def send_events():
    producer.send_batch(batch)
def receive_events():
    consumer.receive(on_event=handler)
def write():
    blob.upload_blob(data)
def read():
    blob.download_blob()
def secret():
    secrets.get_secret('token-python')
def hidden(Bus, Producer, Storage, Secrets):
    Bus(namespace, credential).get_queue_sender('hidden-bus').send_messages(message)
    Producer(namespace, 'hidden-hub', credential).send_batch(batch)
    Storage(endpoint, credential).get_blob_client('hidden-container', 'object').upload_blob(data)
    Secrets(vault, credential).get_secret('hidden-secret')
def handler(message):
    pass

from azure.servicebus.aio import ServiceBusClient as AsyncBus
from azure.eventhub.aio import EventHubProducerClient as AsyncProducer, EventHubConsumerClient as AsyncConsumer
from azure.storage.blob.aio import BlobServiceClient as AsyncStorage
from azure.keyvault.secrets.aio import SecretClient as AsyncSecrets
def topic_sender():
    Bus.from_connection_string(connection).get_topic_sender('factory-topic-python').send_messages(message)
def factory_hub():
    Producer.from_connection_string(connection, eventhub_name='factory-hub-python').send_batch(batch)
def factory_consumer():
    Consumer.from_connection_string(connection, consumer_group='group', eventhub_name='factory-hub-python').receive(handler)
def factory_blob():
    Storage.from_connection_string(connection).get_container_client('factory-container-python').get_blob_client(unknown_object).upload_blob(data)
def container_read():
    storage.get_container_client('factory-container-python').get_blob_client(unknown_object).download_blob()
def context_bus():
    with Bus.from_connection_string(connection) as client:
        with client.get_queue_sender('context-python') as queue:
            queue.send_messages(message)
async def aio_send():
    async with AsyncBus.from_connection_string(connection) as client:
        sender = client.get_queue_sender('aio-python')
        await sender.send_messages(message)
async def aio_receive():
    client = AsyncBus(namespace, credential)
    receiver = client.get_queue_receiver('aio-python')
    await receiver.receive_messages(1)
async def aio_produce():
    client = AsyncProducer(namespace, 'aio-hub-python', credential)
    await client.send_batch(batch)
async def aio_consume():
    client = AsyncConsumer(namespace, 'aio-hub-python', consumer_group='group', credential=credential)
    await client.receive(handler)
async def aio_write():
    client = AsyncStorage(endpoint, credential)
    await client.get_blob_client('aio-container-python', unknown_object).upload_blob(data)
async def aio_read():
    client = AsyncStorage(endpoint, credential)
    await client.get_blob_client('aio-container-python', unknown_object).download_blob()
async def aio_secret():
    await AsyncSecrets(vault, credential).get_secret('aio-secret-python')
