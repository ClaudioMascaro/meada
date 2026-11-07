from google.cloud import pubsub_v1 as bus
def rebound(unknown):
    global bus
    bus = unknown
    bus.PublisherClient().publish('forbidden-global', data)
def outer():
    from google.cloud import pubsub_v1 as local
    def inner(unknown):
        nonlocal local
        local = unknown
        local.PublisherClient().publish('forbidden-nonlocal', data)
    return inner
