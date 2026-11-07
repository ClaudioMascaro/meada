from google.cloud import pubsub_v1 as bus
from google.cloud import storage as store
from google.cloud import secretmanager as secret

publisher = bus.PublisherClient()
subscriber = bus.SubscriberClient()
storage = store.Client()
blob = storage.bucket('uploads-python').blob(unknown_object)
secrets = secret.SecretManagerServiceClient()

def send():
    publisher.publish(publisher.topic_path('acme', 'orders-python'), data)

def receive():
    subscriber.subscribe(subscriber.subscription_path('acme', 'orders-python'), callback=handle)

def write():
    blob.upload_from_string(data)

def read():
    blob.download_as_bytes()

def token():
    secrets.access_secret_version(request={'name': 'projects/acme/secrets/token-python/versions/latest'})

def handle(message):
    pass

def hidden(bus, store, secret):
    publisher = bus.PublisherClient()
    publisher.publish('hidden-topic', data)
    storage = store.Client()
    storage.bucket('hidden-bucket').blob('object').upload_from_string(data)
    secrets = secret.SecretManagerServiceClient()
    secrets.access_secret_version(request={'name': 'hidden-secret'})
