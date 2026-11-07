from google.cloud import pubsub_v1
async def make():
    return pubsub_v1.PublisherClient()
async def Awaited():
    client=await make()
    client.publish('projects/review/topics/await-python', b'data')
async def LateAwait():
    pending=make()
    client=await pending
    client.publish('projects/review/topics/late-python', b'data')
def Unawaited():
    pending=make()
    pending.publish('projects/review/topics/trap-pending-python', b'data')
