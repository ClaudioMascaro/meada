from google.cloud import pubsub_v1 as bus
import os
publisher = bus.PublisherClient()
subscriber = bus.SubscriberClient()
def publishPython():
    publisher.publish(os.environ['TOPIC_ID'], data)
def receivePython():
    subscriber.subscribe(os.environ['SUB_ID'], callback=handler)
