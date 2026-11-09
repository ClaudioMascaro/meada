from google.cloud import pubsub_v1, storage, secretmanager

def TopicCorrect():
    bus=pubsub_v1.PublisherClient()
    bus.publish(pubsub_v1.PublisherClient.topic_path('review','kind-control'),b'x')
def TopicWrongLiteral():
    pubsub_v1.PublisherClient().publish('projects/review/subscriptions/cross-kind',b'x')
def TopicWrongHelper():
    pubsub_v1.PublisherClient().publish(pubsub_v1.SubscriberClient.subscription_path('review','cross-kind'),b'x')
def SubscriptionWrongHelper():
    pubsub_v1.SubscriberClient().subscribe(pubsub_v1.PublisherClient.topic_path('review','cross-kind'),callback=handler)
def BucketWrongHelper():
    storage.Client().bucket(pubsub_v1.PublisherClient.topic_path('review','cross-kind')).blob('x').download_as_bytes()
def SecretWrongHelper():
    secretmanager.SecretManagerServiceClient().access_secret_version(request={'name':pubsub_v1.PublisherClient.topic_path('review','cross-kind')})
def handler(message):pass
