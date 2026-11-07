from google.cloud import pubsub_v1 as bus
from google.cloud import storage as store
from google.cloud import secretmanager as secret
import os

def make(name):
    return store.Client().bucket(name).blob('obj')
a = make('factory-python-a')
b = make('factory-python-b')
def first():
    a.upload_from_string(data)
def second():
    b.upload_from_string(data)
publisher = bus.PublisherClient()
subscriber = bus.SubscriberClient()
storage = store.Client()
secrets = secret.SecretManagerServiceClient()
def env_send():
    publisher.publish(os.environ['TOPIC_PYTHON'], data)
def env_receive():
    subscriber.subscribe(os.environ['SUB_PYTHON'], callback=handler)
def env_bucket():
    storage.bucket(os.environ['BUCKET_PYTHON']).blob(unknown).download_as_bytes()
def env_secret():
    secrets.access_secret_version(request={'name': os.environ['SECRET_PYTHON']})
def handler(message):
    pass

class Factory:
    def make_client(self):
        return bus.PublisherClient()
def guessed(unknown):
    client = unknown.make_client()
    client.publish('forbidden-hop', data)
def mixed(flag, unknown):
    if flag:
        return bus.PublisherClient()
    return unknown
def opaque(flag, unknown):
    client = mixed(flag, unknown)
    client.publish('forbidden-branch', data)
def reassigned(unknown):
    client = bus.PublisherClient()
    client = unknown
    client.publish('forbidden-write', data)
def parameter(bus):
    bus.PublisherClient().publish('forbidden-parameter', data)
def with_name(unknown):
    with unknown as bus:
        bus.PublisherClient().publish('forbidden-with', data)
def except_name():
    try:
        run()
    except Exception as bus:
        bus.PublisherClient().publish('forbidden-except', data)
def walrus(unknown):
    if (bus := unknown):
        bus.PublisherClient().publish('forbidden-walrus', data)
def comprehension(unknown):
    return [bus.PublisherClient().publish('forbidden-comprehension', data) for bus in unknown]
def lam():
    return lambda bus: bus.PublisherClient().publish('forbidden-lambda', data)
def matched(unknown):
    match unknown:
        case {'client': bus}:
            bus.PublisherClient().publish('forbidden-match', data)
def imported():
    import fake as bus
    bus.PublisherClient().publish('forbidden-import', data)
