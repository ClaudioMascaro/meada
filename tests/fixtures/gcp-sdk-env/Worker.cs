using System;
using Google.Cloud.PubSub.V1;
class Worker {
    async Task publishCsharp() { var publisher = await PublisherClient.CreateAsync(Environment.GetEnvironmentVariable("TOPIC_ID")); await publisher.PublishAsync(data); }
    async Task receiveCsharp() { var subscriber = await SubscriberClient.CreateAsync(Environment.GetEnvironmentVariable("SUB_ID")); await subscriber.StartAsync(handler); }
}
