#!/usr/bin/env python3
"""Neutral Azure SDK #670/#642 CLI/MCP flow, with source-driven complete side inventories."""
import argparse
from collections import Counter
import hashlib
import json
import os
from pathlib import Path
import socket
import subprocess
import time

from prove import Http, answer, free_port, git as prove_git, mcp_answers

REMOTE = 'example.com/zz-prove-azure-sdk/'

def git(root, *args):
    return prove_git(*args, cwd=root)


def sources():
    sources={}
    for lang,ext in [('javascript','js'),('typescript','ts')]:
     sources[lang]=(f'worker.{ext}',f'''import {{ ServiceBusClient as Bus }} from '@azure/service-bus';
    import {{ EventHubProducerClient as Producer, EventHubConsumerClient as Consumer }} from '@azure/event-hubs';
    import {{ BlobServiceClient as Storage }} from '@azure/storage-blob';
    import {{ SecretClient as Secrets }} from '@azure/keyvault-secrets';
    const bus = new Bus(connection);
    const sender = bus.createSender('orders-{lang}');
    const receiver = bus.createReceiver('orders-{lang}');
    const subscription = bus.createReceiver('orders-{lang}', 'audit');
    const producer = new Producer(connection, 'events-{lang}');
    const consumer = new Consumer('group', connection, 'events-{lang}');
    const storage = new Storage(endpoint, credential);
    const container = storage.getContainerClient('uploads-{lang}');
    const blob = container.getBlockBlobClient(unknownObject);
    const secrets = new Secrets(vault, credential);
    export function send() {{ sender.sendMessages(message); }}
    export function receive() {{ receiver.receiveMessages(1); }}
    export function subscribe() {{ subscription.subscribe({{processMessage: handler}}); }}
    export function sendEvents() {{ producer.sendBatch(batch); }}
    export function receiveEvents() {{ consumer.subscribe({{processEvents: handler}}); }}
    export function write() {{ blob.upload(data, count); }}
    export function read() {{ container.getBlobClient(unknownObject).download(); }}
    export function secret() {{ secrets.getSecret('token-{lang}'); }}
    export function hidden(Bus, Producer, Storage, Secrets) {{
     new Bus(connection).createSender('hidden-bus').sendMessages(message);
     new Producer(connection, 'hidden-hub').sendBatch(batch);
     new Storage(endpoint, credential).getContainerClient('hidden-container').getBlockBlobClient('object').upload(data, count);
     new Secrets(vault, credential).getSecret('hidden-secret');
    }}
    function handler(message) {{}}
    ''')
    sources['python']=('worker.py','''from azure.servicebus import ServiceBusClient as Bus
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
    ''')
    sources['java']=('Worker.java','''import com.azure.messaging.servicebus.ServiceBusClientBuilder;
    import com.azure.messaging.eventhubs.EventHubClientBuilder;
    import com.azure.storage.blob.BlobServiceClientBuilder;
    import com.azure.security.keyvault.secrets.SecretClientBuilder;
    class Worker {
     void send() { new ServiceBusClientBuilder().connectionString(connection).sender().queueName("orders-java").buildClient().sendMessage(message); }
     void receive() { new ServiceBusClientBuilder().connectionString(connection).receiver().queueName("orders-java").buildClient().receiveMessages(1); }
     void subscribe() { new ServiceBusClientBuilder().connectionString(connection).receiver().topicName("orders-java").subscriptionName("audit").buildClient().receiveMessages(1); }
     void sendEvents() { new EventHubClientBuilder().connectionString(connection,"events-java").buildProducerClient().send(batch); }
     void receiveEvents() { new EventHubClientBuilder().connectionString(connection,"events-java").consumerGroup("group").buildConsumerClient().receiveFromPartition("0",1,position); }
     void write() { new BlobServiceClientBuilder().endpoint(endpoint).credential(credential).buildClient().getBlobContainerClient("uploads-java").getBlobClient(object).upload(data); }
     void read() { new BlobServiceClientBuilder().endpoint(endpoint).buildClient().getBlobContainerClient("uploads-java").getBlobClient(object).downloadContent(); }
     void secret() { new SecretClientBuilder().vaultUrl(vault).credential(credential).buildClient().getSecret("token-java"); }
    }
    ''')
    sources['go']=('worker.go','''package worker
    import (
     bus "github.com/Azure/azure-sdk-for-go/sdk/messaging/azservicebus"
     hub "github.com/Azure/azure-sdk-for-go/sdk/messaging/azeventhubs"
     blob "github.com/Azure/azure-sdk-for-go/sdk/storage/azblob"
     secrets "github.com/Azure/azure-sdk-for-go/sdk/security/keyvault/azsecrets"
    )
    func run() {
     client, err := bus.NewClient(namespace, credential, nil)
     sender, err := client.NewSender("orders-go", nil)
     receiver, err := client.NewReceiverForQueue("orders-go", nil)
     subscription, err := client.NewReceiverForSubscription("orders-go", "audit", nil)
     sender.SendMessage(ctx, message, nil)
     receiver.ReceiveMessages(ctx, 1, nil)
     subscription.ReceiveMessages(ctx, 1, nil)
     producer, err := hub.NewProducerClient(namespace, "events-go", credential, nil)
     consumer, err := hub.NewConsumerClient(namespace, "events-go", "group", credential, nil)
     partition, err := consumer.NewPartitionClient("0", nil)
     producer.SendEventDataBatch(ctx, batch, nil)
     partition.ReceiveEvents(ctx, 1, nil)
     storage, err := blob.NewClient(endpoint, credential, nil)
     storage.UploadBuffer(ctx, "uploads-go", object, data, nil)
     storage.DownloadBuffer(ctx, "uploads-go", object, data, nil)
     secret, err := secrets.NewClient(vault, credential, nil)
     secret.GetSecret(ctx, "token-go", "", nil)
    }
    ''')
    sources['csharp']=('Worker.cs','''using Azure.Messaging.ServiceBus;
    using Azure.Messaging.EventHubs.Producer;
    using Azure.Messaging.EventHubs.Consumer;
    using Azure.Storage.Blobs;
    using Azure.Security.KeyVault.Secrets;
    class Worker {
     async Task Send() { var bus = new ServiceBusClient(connection); var sender = bus.CreateSender("orders-csharp"); await sender.SendMessageAsync(message); }
     async Task Receive() { var bus = new ServiceBusClient(connection); var receiver = bus.CreateReceiver("orders-csharp"); await receiver.ReceiveMessageAsync(); }
     async Task Subscribe() { var bus = new ServiceBusClient(connection); var receiver = bus.CreateReceiver("orders-csharp", "audit"); await receiver.ReceiveMessagesAsync(1); }
     async Task SendEvents() { var producer = new EventHubProducerClient(connection, "events-csharp"); await producer.SendAsync(batch); }
     async Task ReceiveEvents() { var consumer = new EventHubConsumerClient("group", connection, "events-csharp"); consumer.ReadEventsAsync(); }
     async Task Write() { var storage = new BlobServiceClient(connection); var container = storage.GetBlobContainerClient("uploads-csharp"); var blob = container.GetBlobClient(unknownObject); await blob.UploadAsync(data); }
     async Task Read() { var storage = new BlobServiceClient(connection); var blob = storage.GetBlobContainerClient("uploads-csharp").GetBlobClient(unknownObject); await blob.DownloadToAsync(stream); }
     async Task Secret() { var secret = new SecretClient(vault, credential); await secret.GetSecretAsync("token-csharp"); }
    }
    ''')
    return {lang: (file, "\n".join(line[4:] if line.startswith("    ") else line for line in content.splitlines()) + "\n") for lang, (file, content) in sources.items()}


EXTRA_SOURCES = {'origins': {'worker.ts': 'import { ServiceBusClient as Bus, ServiceBusSender } from '
                          "'@azure/service-bus';\n"
                          'import { EventHubProducerClient as Producer } from '
                          "'@azure/event-hubs';\n"
                          "import { BlobServiceClient as Storage } from '@azure/storage-blob';\n"
                          "import { SecretClient as Secrets } from '@azure/keyvault-secrets';\n"
                          'const bus = new Bus(connection);\n'
                          'function sender(name) { return bus.createSender(name); }\n'
                          'function receiver({topic, sub}) { return bus.createReceiver(topic, '
                          'sub); }\n'
                          'export function run() {\n'
                          " sender('factory-first-typescript').sendMessages(message);\n"
                          " sender('factory-second-typescript').sendMessages(message);\n"
                          " receiver({topic:'topic-first-typescript', "
                          "sub:'audit'}).receiveMessages(1);\n"
                          " receiver({topic:'topic-second-typescript', "
                          "sub:'ops'}).receiveMessages(1);\n"
                          '}\n'
                          'export function hidden(Bus, Producer, Storage, Secrets) {\n'
                          ' new '
                          "Bus(connection).createSender('hidden-bus-typescript').sendMessages(message);\n"
                          " new Producer(connection, 'hidden-hub-typescript').sendBatch(batch);\n"
                          ' new Storage(endpoint, '
                          "credential).getContainerClient('hidden-container-typescript').getBlockBlobClient('object').upload(data, "
                          'count);\n'
                          " new Secrets(vault, credential).getSecret('hidden-secret-typescript');\n"
                          '}\n'
                          'export function typed(client: ServiceBusSender) { '
                          'client.sendMessages(message); }\n'
                          'export function reassigned() {\n'
                          " let client = bus.createSender('stale-typescript'); client = opaque;\n"
                          ' client.sendMessages(message);\n'
                          '}\n'
                          'function alternative(flag) { if (flag) return '
                          "bus.createSender('unsafe-typescript'); return opaque; }\n"
                          'export function opaquePath() { alternative(flag).sendMessages(message); '
                          '}\n'
                          'export function nameMatched() { '
                          "foreign.sender('matched-typescript').sendMessages(message); }\n"
                          'export function unsupported() { '
                          "bus.createSender('unsupported-typescript').inventedSend(message); }\n"
                          'export function handleOnly() { return '
                          "bus.createSender('handle-only-typescript'); }\n"
                          'export function unaffected() { '
                          "bus.createSender('unaffected-typescript').sendMessages(message); }\n"
                          '\n'
                          'class Factory { makeClient() { return new Bus(connection); } }\n'
                          'export function guessName(unknown) { '
                          "unknown.makeClient().createSender('matched-constructor').sendMessages(message); "
                          '}\n',
             'worker.go': 'package worker\n'
                          'import bus '
                          '"github.com/Azure/azure-sdk-for-go/sdk/messaging/azservicebus"\n'
                          'func sender(name string) *bus.Sender { client, _ := '
                          'bus.NewClient(namespace, credential, nil); result, _ := '
                          'client.NewSender(name, nil); return result }\n'
                          'func receiver(topic string, sub string) *bus.Receiver { client, _ := '
                          'bus.NewClient(namespace, credential, nil); result, _ := '
                          'client.NewReceiverForSubscription(topic, sub, nil); return result }\n'
                          'func run() {\n'
                          ' sender("factory-first-go").SendMessage(ctx, message, nil)\n'
                          ' sender("factory-second-go").SendMessage(ctx, message, nil)\n'
                          ' receiver("topic-first-go", "audit").ReceiveMessages(ctx, 1, nil)\n'
                          ' receiver("topic-second-go", "ops").ReceiveMessages(ctx, 1, nil)\n'
                          '}\n'
                          'func typed(client *bus.Sender) { client.SendMessage(ctx, message, nil) '
                          '}\n'
                          'func hidden(bus any) { client, _ := bus.NewClient(namespace, '
                          'credential, nil); result, _ := client.NewSender("hidden-go", nil); '
                          'result.SendMessage(ctx, message, nil) }\n'
                          'func reassigned() { client := sender("stale-go"); client = opaque; '
                          'client.SendMessage(ctx, message, nil) }\n'
                          'func alternative(flag bool) *bus.Sender { if flag { return '
                          'sender("unsafe-go") }; return opaque }\n'
                          'func opaquePath() { alternative(flag).SendMessage(ctx, message, nil) }\n'
                          'func nameMatched() { foreign.sender("matched-go").SendMessage(ctx, '
                          'message, nil) }\n'
                          'func unsupported() { sender("unsupported-go").InventedSend(ctx, '
                          'message, nil) }\n'
                          'func handleOnly() *bus.Sender { return sender("handle-only-go") }\n'
                          'func unaffected() { client, _ := bus.NewClient(namespace, credential, '
                          'nil); result, _ := client.NewSender("unaffected-go", nil); '
                          'result.SendMessage(ctx, message, nil) }\n'
                          '\n'
                          'type Factory struct{}\n'
                          'func (f Factory) makeClient() *bus.Client { client, _ := '
                          'bus.NewClient(namespace,credential,nil); return client }\n'
                          'func guessName(unknown interface{}) { client := unknown.makeClient(); '
                          'result, _ := client.NewSender("matched-constructor",nil); '
                          'result.SendMessage(ctx,message,nil) }\n',
             'worker.js': 'import { ServiceBusClient as Bus, ServiceBusSender } from '
                          "'@azure/service-bus';\n"
                          'import { EventHubProducerClient as Producer } from '
                          "'@azure/event-hubs';\n"
                          "import { BlobServiceClient as Storage } from '@azure/storage-blob';\n"
                          "import { SecretClient as Secrets } from '@azure/keyvault-secrets';\n"
                          'const bus = new Bus(connection);\n'
                          'function sender(name) { return bus.createSender(name); }\n'
                          'function receiver({topic, sub}) { return bus.createReceiver(topic, '
                          'sub); }\n'
                          'export function run() {\n'
                          " sender('factory-first-javascript').sendMessages(message);\n"
                          " sender('factory-second-javascript').sendMessages(message);\n"
                          " receiver({topic:'topic-first-javascript', "
                          "sub:'audit'}).receiveMessages(1);\n"
                          " receiver({topic:'topic-second-javascript', "
                          "sub:'ops'}).receiveMessages(1);\n"
                          '}\n'
                          'export function hidden(Bus, Producer, Storage, Secrets) {\n'
                          ' new '
                          "Bus(connection).createSender('hidden-bus-javascript').sendMessages(message);\n"
                          " new Producer(connection, 'hidden-hub-javascript').sendBatch(batch);\n"
                          ' new Storage(endpoint, '
                          "credential).getContainerClient('hidden-container-javascript').getBlockBlobClient('object').upload(data, "
                          'count);\n'
                          " new Secrets(vault, credential).getSecret('hidden-secret-javascript');\n"
                          '}\n'
                          'export function typed(client) { client.sendMessages(message); }\n'
                          'export function reassigned() {\n'
                          " let client = bus.createSender('stale-javascript'); client = opaque;\n"
                          ' client.sendMessages(message);\n'
                          '}\n'
                          'function alternative(flag) { if (flag) return '
                          "bus.createSender('unsafe-javascript'); return opaque; }\n"
                          'export function opaquePath() { alternative(flag).sendMessages(message); '
                          '}\n'
                          'export function nameMatched() { '
                          "foreign.sender('matched-javascript').sendMessages(message); }\n"
                          'export function unsupported() { '
                          "bus.createSender('unsupported-javascript').inventedSend(message); }\n"
                          'export function handleOnly() { return '
                          "bus.createSender('handle-only-javascript'); }\n"
                          'export function unaffected() { '
                          "bus.createSender('unaffected-javascript').sendMessages(message); }\n"
                          '\n'
                          'class Factory { makeClient() { return new Bus(connection); } }\n'
                          'export function guessName(unknown) { '
                          "unknown.makeClient().createSender('matched-constructor').sendMessages(message); "
                          '}\n',
             'Worker.cs': 'using Azure.Messaging.ServiceBus;\n'
                          'using Azure.Messaging.EventHubs.Producer;\n'
                          'using Azure.Storage.Blobs;\n'
                          'using Azure.Security.KeyVault.Secrets;\n'
                          'class Worker {\n'
                          ' ServiceBusReceiver Receiver(string topic, string sub) { return new '
                          'ServiceBusClient(connection).CreateReceiver(topic, sub); }\n'
                          ' async Task Run() {\n'
                          '  await Receiver("topic-first-csharp", '
                          '"audit").ReceiveMessagesAsync(1);\n'
                          '  await Receiver("topic-second-csharp", '
                          '"ops").ReceiveMessagesAsync(1);\n'
                          ' }\n'
                          ' async Task Typed(ServiceBusReceiver client) { await '
                          'client.ReceiveMessagesAsync(1); }\n'
                          ' async Task Reassigned() { var client = Receiver("stale-csharp", '
                          '"audit"); client = opaque; await client.ReceiveMessagesAsync(1); }\n'
                          ' ServiceBusReceiver Alternative(bool flag) { if(flag) return '
                          'Receiver("unsafe-csharp", "audit"); return opaque; }\n'
                          ' async Task OpaquePath() { await '
                          'Alternative(flag).ReceiveMessagesAsync(1); }\n'
                          ' async Task NameMatched() { await foreign.Receiver("matched-csharp", '
                          '"audit").ReceiveMessagesAsync(1); }\n'
                          ' void HandleOnly() { new '
                          'ServiceBusClient(connection).CreateReceiver("handle-only-csharp", '
                          '"audit"); }\n'
                          ' async Task Unsupported() { await new '
                          'ServiceBusClient(connection).CreateReceiver("unsupported-csharp").InventedReceive(); '
                          '}\n'
                          ' async Task Unaffected() { await new '
                          'ServiceBusClient(connection).CreateReceiver("unaffected-csharp").ReceiveMessageAsync(); '
                          '}\n'
                          ' async Task Hidden(ServiceBusClient ServiceBusClient, '
                          'EventHubProducerClient EventHubProducerClient, BlobServiceClient '
                          'BlobServiceClient, SecretClient SecretClient) {\n'
                          '  await new '
                          'ServiceBusClient(connection).CreateReceiver("hidden-csharp").ReceiveMessageAsync();\n'
                          '  await new EventHubProducerClient(connection, '
                          '"hidden-hub-csharp").SendAsync(batch);\n'
                          '  await new '
                          'BlobServiceClient(connection).GetBlobContainerClient("hidden-blob-csharp").UploadBlobAsync("object", '
                          'data);\n'
                          '  await new SecretClient(vault, '
                          'credential).GetSecretAsync("hidden-secret-csharp");\n'
                          ' }\n'
                          '}\n',
             'worker.py': 'from azure.servicebus import ServiceBusClient as Bus, ServiceBusSender\n'
                          'from azure.eventhub import EventHubProducerClient as Producer\n'
                          'from azure.storage.blob import BlobServiceClient as Storage\n'
                          'from azure.keyvault.secrets import SecretClient as Secrets\n'
                          'bus = Bus(namespace, credential)\n'
                          'def sender(name):\n'
                          '    return bus.get_queue_sender(name)\n'
                          'def receiver(topic, sub):\n'
                          '    return bus.get_subscription_receiver(topic, sub)\n'
                          'def run():\n'
                          "    sender('factory-first-python').send_messages(message)\n"
                          "    sender('factory-second-python').send_messages(message)\n"
                          "    receiver('topic-first-python', 'audit').receive_messages(1)\n"
                          "    receiver('topic-second-python', 'ops').receive_messages(1)\n"
                          'def hidden(Bus, Producer, Storage, Secrets):\n'
                          '    Bus(namespace, '
                          "credential).get_queue_sender('hidden-bus-python').send_messages(message)\n"
                          "    Producer(namespace, 'hidden-hub-python', "
                          'credential).send_batch(batch)\n'
                          '    Storage(endpoint, '
                          "credential).get_blob_client('hidden-container-python', "
                          "'object').upload_blob(data)\n"
                          "    Secrets(vault, credential).get_secret('hidden-secret-python')\n"
                          'def typed(client: ServiceBusSender):\n'
                          '    client.send_messages(message)\n'
                          'def reassigned():\n'
                          "    client = bus.get_queue_sender('stale-python')\n"
                          '    client = opaque\n'
                          '    client.send_messages(message)\n'
                          'def alternative(flag):\n'
                          '    if flag:\n'
                          "        return bus.get_queue_sender('unsafe-python')\n"
                          '    return opaque\n'
                          'def opaque_path():\n'
                          '    alternative(flag).send_messages(message)\n'
                          'def name_matched():\n'
                          "    foreign.sender('matched-python').send_messages(message)\n"
                          'def unsupported():\n'
                          "    bus.get_queue_sender('unsupported-python').invented_send(message)\n"
                          'def handle_only():\n'
                          "    return bus.get_queue_sender('handle-only-python')\n"
                          'def unaffected():\n'
                          "    bus.get_queue_sender('unaffected-python').send_messages(message)\n"
                          '\n'
                          'class Factory:\n'
                          '    def make_client(self):\n'
                          '        return Bus(namespace, credential)\n'
                          'def guess_name(unknown):\n'
                          '    '
                          "unknown.make_client().get_queue_sender('matched-constructor').send_messages(message)\n",
             'Worker.java': 'import com.azure.messaging.servicebus.ServiceBusClientBuilder;\n'
                            'import com.azure.messaging.servicebus.ServiceBusSenderClient;\n'
                            'import com.azure.messaging.servicebus.ServiceBusReceiverClient;\n'
                            'class Worker {\n'
                            ' ServiceBusSenderClient sender(String name) { return new '
                            'ServiceBusClientBuilder().connectionString(connection).sender().queueName(name).buildClient(); '
                            '}\n'
                            ' ServiceBusReceiverClient receiver(String topic, String sub) { return '
                            'new '
                            'ServiceBusClientBuilder().connectionString(connection).receiver().topicName(topic).subscriptionName(sub).buildClient(); '
                            '}\n'
                            ' void run() {\n'
                            '  sender("factory-first-java").sendMessage(message);\n'
                            '  sender("factory-second-java").sendMessage(message);\n'
                            '  receiver("topic-first-java", "audit").receiveMessages(1);\n'
                            '  receiver("topic-second-java", "ops").receiveMessages(1);\n'
                            ' }\n'
                            ' void typed(ServiceBusSenderClient client) { '
                            'client.sendMessage(message); }\n'
                            ' void reassigned() { var client = sender("stale-java"); client = '
                            'opaque; client.sendMessage(message); }\n'
                            ' ServiceBusSenderClient alternative(boolean flag) { if(flag) return '
                            'sender("unsafe-java"); return opaque; }\n'
                            ' void opaquePath() { alternative(flag).sendMessage(message); }\n'
                            ' void nameMatched() { '
                            'foreign.sender("matched-java").sendMessage(message); }\n'
                            ' void unsupported() { '
                            'sender("unsupported-java").inventedSend(message); }\n'
                            ' ServiceBusSenderClient handleOnly() { return '
                            'sender("handle-only-java"); }\n'
                            ' void unaffected() { new '
                            'ServiceBusClientBuilder().connectionString(connection).sender().queueName("unaffected-java").buildClient().sendMessage(message); '
                            '}\n'
                            ' class Hidden {\n'
                            '  class ServiceBusClientBuilder {}\n'
                            '  void shadowed() { new '
                            'ServiceBusClientBuilder().connectionString(connection).sender().queueName("hidden-java").buildClient().sendMessage(message); '
                            '}\n'
                            ' }\n'
                            '}\n'},
 'environment': {'worker.go': 'package worker\n'
                              'import "os"\n'
                              'import bus '
                              '"github.com/Azure/azure-sdk-for-go/sdk/messaging/azservicebus"\n'
                              'func receive() { client, _ := bus.NewClient(namespace, credential, '
                              'nil); receiver, _ := '
                              'client.NewReceiverForSubscription(os.Getenv("TOPIC"), '
                              'os.Getenv("SUB"), nil); receiver.ReceiveMessages(ctx, 1, nil) }\n'
                              'func queue() { client, _ := bus.NewClient(namespace, credential, '
                              'nil); receiver, _ := client.NewReceiverForQueue(os.Getenv("QUEUE"), '
                              'nil); receiver.ReceiveMessages(ctx, 1, nil) }\n'
                              'func literalTopic() { client, _ := bus.NewClient(namespace, '
                              'credential, nil); receiver, _ := '
                              'client.NewReceiverForSubscription("literal", os.Getenv("SUB"), '
                              'nil); receiver.ReceiveMessages(ctx, 1, nil) }\n'
                              'func literalSub() { client, _ := bus.NewClient(namespace, '
                              'credential, nil); receiver, _ := '
                              'client.NewReceiverForSubscription(os.Getenv("TOPIC"), "fixed", '
                              'nil); receiver.ReceiveMessages(ctx, 1, nil) }\n'
                              'func unread() { client, _ := bus.NewClient(namespace, credential, '
                              'nil); receiver, _ := '
                              'client.NewReceiverForSubscription(os.Getenv("TOPIC"), '
                              'os.Getenv("UNREAD"), nil); receiver.ReceiveMessages(ctx, 1, nil) }\n'
                              '\n'
                              'func literalQueue() { client, _ := '
                              'bus.NewClient(namespace,credential,nil); receiver, _ := '
                              'client.NewReceiverForQueue("unaffected-go",nil); '
                              'receiver.ReceiveMessages(ctx,1,nil) }\n',
                 'worker.js': "import { ServiceBusClient } from '@azure/service-bus';\n"
                              'const bus = new ServiceBusClient(connection);\n'
                              'export function receive() { bus.createReceiver(process.env.TOPIC, '
                              'process.env.SUB).receiveMessages(1); }\n'
                              'export function queue() { '
                              'bus.createReceiver(process.env.QUEUE).receiveMessages(1); }\n'
                              "export function literalTopic() { bus.createReceiver('literal', "
                              'process.env.SUB).receiveMessages(1); }\n'
                              'export function literalSub() { '
                              "bus.createReceiver(process.env.TOPIC, 'fixed').receiveMessages(1); "
                              '}\n'
                              'export function unread() { bus.createReceiver(process.env.TOPIC, '
                              'process.env.UNREAD).receiveMessages(1); }\n'
                              '\n'
                              'export function literalQueue() { '
                              "bus.createReceiver('unaffected-javascript').receiveMessages(1); }\n"
                              'function constructedReceiver({topic,sub}) { return '
                              'bus.createReceiver(topic,sub); }\n'
                              'export function mutablePair() { const '
                              'names={topic:process.env.TOPIC,sub:process.env.SUB}; '
                              'names.sub=runtime; constructedReceiver(names).receiveMessages(1); '
                              '}\n',
                 'Worker.cs': 'using System;\n'
                              'using Azure.Messaging.ServiceBus;\n'
                              'class EnvWorker {\n'
                              ' async Task Receive() { await new '
                              'ServiceBusClient(connection).CreateReceiver(Environment.GetEnvironmentVariable("TOPIC"), '
                              'Environment.GetEnvironmentVariable("SUB")).ReceiveMessagesAsync(1); '
                              '}\n'
                              ' async Task Queue() { await new '
                              'ServiceBusClient(connection).CreateReceiver(Environment.GetEnvironmentVariable("QUEUE")).ReceiveMessagesAsync(1); '
                              '}\n'
                              ' async Task LiteralTopic() { await new '
                              'ServiceBusClient(connection).CreateReceiver("literal", '
                              'Environment.GetEnvironmentVariable("SUB")).ReceiveMessagesAsync(1); '
                              '}\n'
                              ' async Task LiteralSub() { await new '
                              'ServiceBusClient(connection).CreateReceiver(Environment.GetEnvironmentVariable("TOPIC"), '
                              '"fixed").ReceiveMessagesAsync(1); }\n'
                              ' async Task Unread() { await new '
                              'ServiceBusClient(connection).CreateReceiver(Environment.GetEnvironmentVariable("TOPIC"), '
                              'Environment.GetEnvironmentVariable("UNREAD")).ReceiveMessagesAsync(1); '
                              '}\n'
                              ' async Task LiteralQueue() { await new '
                              'ServiceBusClient(connection).CreateReceiver("unaffected-csharp").ReceiveMessageAsync(); '
                              '}\n'
                              '}\n',
                 'worker.py': 'import os\n'
                              'from azure.servicebus import ServiceBusClient\n'
                              'bus = ServiceBusClient(namespace, credential)\n'
                              'def receive():\n'
                              "    bus.get_subscription_receiver(os.getenv('TOPIC'), "
                              "os.getenv('SUB')).receive_messages(1)\n"
                              'def queue():\n'
                              "    bus.get_queue_receiver(os.getenv('QUEUE')).receive_messages(1)\n"
                              'def literal_topic():\n'
                              "    bus.get_subscription_receiver('literal', "
                              "os.getenv('SUB')).receive_messages(1)\n"
                              'def literal_sub():\n'
                              "    bus.get_subscription_receiver(os.getenv('TOPIC'), "
                              "'fixed').receive_messages(1)\n"
                              'def unread():\n'
                              "    bus.get_subscription_receiver(os.getenv('TOPIC'), "
                              "os.getenv('UNREAD')).receive_messages(1)\n"
                              '\n'
                              'def literal_queue():\n'
                              '    '
                              "bus.get_queue_receiver('unaffected-python').receive_messages(1)\n",
                 'Worker.java': 'import com.azure.messaging.servicebus.ServiceBusClientBuilder;\n'
                                'class EnvWorker {\n'
                                ' void receive() { new '
                                'ServiceBusClientBuilder().connectionString(connection).receiver().topicName(System.getenv("TOPIC")).subscriptionName(System.getenv("SUB")).buildClient().receiveMessages(1); '
                                '}\n'
                                ' void queue() { new '
                                'ServiceBusClientBuilder().connectionString(connection).receiver().queueName(System.getenv("QUEUE")).buildClient().receiveMessages(1); '
                                '}\n'
                                ' void literalTopic() { new '
                                'ServiceBusClientBuilder().connectionString(connection).receiver().topicName("literal").subscriptionName(System.getenv("SUB")).buildClient().receiveMessages(1); '
                                '}\n'
                                ' void literalSub() { new '
                                'ServiceBusClientBuilder().connectionString(connection).receiver().topicName(System.getenv("TOPIC")).subscriptionName("fixed").buildClient().receiveMessages(1); '
                                '}\n'
                                ' void unread() { new '
                                'ServiceBusClientBuilder().connectionString(connection).receiver().topicName(System.getenv("TOPIC")).subscriptionName(System.getenv("UNREAD")).buildClient().receiveMessages(1); '
                                '}\n'
                                ' void literalQueue() { new '
                                'ServiceBusClientBuilder().connectionString(connection).receiver().queueName("unaffected-java").buildClient().receiveMessages(1); '
                                '}\n'
                                '}\n',
                 '.github/workflows/build.yml': 'jobs:\n'
                                                '  publish:\n'
                                                '    steps:\n'
                                                '      - uses: docker/build-push-action@v6\n'
                                                '        with:\n'
                                                '          tags: '
                                                'ghcr.io/probe/azure-sdk-worker:v1\n',
                 'pods.yaml': 'apiVersion: v1\n'
                              'kind: Pod\n'
                              'metadata: {name: sdk-workers}\n'
                              'spec:\n'
                              '  containers:\n'
                              '    - name: first\n'
                              '      image: ghcr.io/probe/azure-sdk-worker:v1\n'
                              '      env:\n'
                              '        - {name: TOPIC, value: orders}\n'
                              '        - {name: SUB, value: audit}\n'
                              '        - name: UNREAD\n'
                              '          valueFrom: {secretKeyRef: {name: private, key: sub}}\n'
                              '        - {name: QUEUE, value: orders/subscriptions/audit}\n'
                              '    - name: second\n'
                              '      image: ghcr.io/probe/azure-sdk-worker:v1\n'
                              '      env:\n'
                              '        - {name: TOPIC, value: shipping}\n'
                              '        - {name: SUB, value: ops}\n'
                              '        - name: UNREAD\n'
                              '          valueFrom: {secretKeyRef: {name: private, key: sub}}\n'
                              '        - {name: QUEUE, value: shipping/subscriptions/ops}\n'
                              '    - name: unread-subscription\n'
                              '      image: ghcr.io/probe/azure-sdk-worker:v1\n'
                              '      env:\n'
                              '        - {name: TOPIC, value: blocked}\n'
                              '        - name: SUB\n'
                              '          valueFrom: {secretKeyRef: {name: private, key: sub}}\n'
                              '        - {name: QUEUE, value: blocked/subscriptions/secret}\n'
                              '---\n'
                              'apiVersion: v1\n'
                              'kind: Secret\n'
                              'metadata: {name: private}\n'
                              'stringData: {sub: secret-subscription-canary-670}\n',
                 'worker.ts': "import { ServiceBusClient } from '@azure/service-bus';\n"
                              'const bus = new '
                              'ServiceBusClient("Endpoint=sb://sdk-canary.servicebus.windows.net/;SharedAccessKeyName=owner;SharedAccessKey=AZURE_SDK_CREDENTIAL_CANARY_670");\n'
                              'export function receive() { bus.createReceiver(process.env.TOPIC, '
                              'process.env.SUB).receiveMessages(1); }\n'
                              'export function queue() { '
                              'bus.createReceiver(process.env.QUEUE).receiveMessages(1); }\n'
                              'export function invalidName() { '
                              "bus.createReceiver('Endpoint=sb://sdk-canary.servicebus.windows.net/;SharedAccessKeyName=owner;SharedAccessKey=AZURE_SDK_CREDENTIAL_CANARY_670').receiveMessages(1); "
                              '}\n'
                              '\n'
                              'export function literalQueue() { '
                              "bus.createReceiver('unaffected-typescript').receiveMessages(1); "
                              '}\n'},
 'variants': {'worker.ts': "import { ServiceBusClient as Bus } from '@azure/service-bus';\n"
                           'import { EventHubProducerClient as Producer, EventHubConsumerClient as '
                           "Consumer } from '@azure/event-hubs';\n"
                           "import { BlobServiceClient as Storage } from '@azure/storage-blob';\n"
                           "import { SecretClient as Secrets } from '@azure/keyvault-secrets';\n"
                           'const bus = new Bus(connection);\n'
                           "const sender = bus.createSender('orders-typescript');\n"
                           "const receiver = bus.createReceiver('orders-typescript');\n"
                           "const subscription = bus.createReceiver('orders-typescript', "
                           "'audit');\n"
                           "const producer = new Producer(connection, 'events-typescript');\n"
                           "const consumer = new Consumer('group', connection, "
                           "'events-typescript');\n"
                           'const storage = new Storage(endpoint, credential);\n'
                           "const container = storage.getContainerClient('uploads-typescript');\n"
                           'const blob = container.getBlockBlobClient(unknownObject);\n'
                           'const secrets = new Secrets(vault, credential);\n'
                           'export function send() { sender.sendMessages(message); }\n'
                           'export function receive() { receiver.receiveMessages(1); }\n'
                           'export function subscribe() { subscription.subscribe({processMessage: '
                           'handler}); }\n'
                           'export function sendEvents() { producer.sendBatch(batch); }\n'
                           'export function receiveEvents() { consumer.subscribe({processEvents: '
                           'handler}); }\n'
                           'export function write() { blob.upload(data, count); }\n'
                           'export function read() { '
                           'container.getBlobClient(unknownObject).download(); }\n'
                           "export function secret() { secrets.getSecret('token-typescript'); }\n"
                           'export function hidden(Bus, Producer, Storage, Secrets) {\n'
                           ' new '
                           "Bus(connection).createSender('hidden-bus').sendMessages(message);\n"
                           " new Producer(connection, 'hidden-hub').sendBatch(batch);\n"
                           ' new Storage(endpoint, '
                           "credential).getContainerClient('hidden-container').getBlockBlobClient('object').upload(data, "
                           'count);\n'
                           " new Secrets(vault, credential).getSecret('hidden-secret');\n"
                           '}\n'
                           'function handler(message) {}\n'
                           '\n'
                           'export function writeData() { blob.uploadData(data); }\n'
                           'export function writeFile() { blob.uploadFile(file); }\n'
                           'export function writeStream() { blob.uploadStream(stream); }\n'
                           'export function readBuffer() { blob.downloadToBuffer(); }\n'
                           'export function readFile() { blob.downloadToFile(file); }\n'
                           'export function staticStorage() { '
                           "Storage.fromConnectionString(connection).getContainerClient('factory-storage').getBlockBlobClient(unknownObject).uploadData(data); "
                           '}\n'
                           'export function structuredQueue() { '
                           "bus.createSender('/subscriptions/s/resourceGroups/g/providers/Microsoft.ServiceBus/namespaces/n/queues/structured').sendMessages(message); "
                           '}\n'
                           'export function structuredSub() { '
                           "bus.createReceiver('/subscriptions/s/resourceGroups/g/providers/Microsoft.ServiceBus/namespaces/n/topics/structured', "
                           "'/subscriptions/s/resourceGroups/g/providers/Microsoft.ServiceBus/namespaces/n/topics/structured/subscriptions/audit').receiveMessages(1); "
                           '}\n'
                           'export function structuredBlob() { '
                           "storage.getContainerClient('https://account.blob.core.windows.net/structured/object?sv=version').getBlobClient(unknownObject).download(); "
                           '}\n'
                           'export function structuredSecret() { '
                           "secrets.getSecret('https://keys.vault.azure.net/secrets/structured/version'); "
                           '}\n'
                           'export function unknownHub() { new '
                           'Producer(connection).sendBatch(batch); }\n'
                           'export function wrongService() { '
                           "bus.createSender('/subscriptions/s/resourceGroups/g/providers/Microsoft.EventHub/namespaces/n/eventhubs/wrong').sendMessages(message); "
                           '}\n'
                           '\n'
                           'export function repeatReads() { blob.download(); blob.download(); '
                           "secrets.getSecret('repeat'); secrets.getSecret('repeat'); }\n",
              'worker.go': 'package worker\n'
                           'import (\n'
                           ' bus "github.com/Azure/azure-sdk-for-go/sdk/messaging/azservicebus"\n'
                           ' hub "github.com/Azure/azure-sdk-for-go/sdk/messaging/azeventhubs"\n'
                           ' blob "github.com/Azure/azure-sdk-for-go/sdk/storage/azblob"\n'
                           ' secrets '
                           '"github.com/Azure/azure-sdk-for-go/sdk/security/keyvault/azsecrets"\n'
                           ')\n'
                           'func run() {\n'
                           ' client, err := bus.NewClient(namespace, credential, nil)\n'
                           ' sender, err := client.NewSender("orders-go", nil)\n'
                           ' receiver, err := client.NewReceiverForQueue("orders-go", nil)\n'
                           ' subscription, err := client.NewReceiverForSubscription("orders-go", '
                           '"audit", nil)\n'
                           ' sender.SendMessage(ctx, message, nil)\n'
                           ' receiver.ReceiveMessages(ctx, 1, nil)\n'
                           ' subscription.ReceiveMessages(ctx, 1, nil)\n'
                           ' producer, err := hub.NewProducerClient(namespace, "events-go", '
                           'credential, nil)\n'
                           ' consumer, err := hub.NewConsumerClient(namespace, "events-go", '
                           '"group", credential, nil)\n'
                           ' partition, err := consumer.NewPartitionClient("0", nil)\n'
                           ' producer.SendEventDataBatch(ctx, batch, nil)\n'
                           ' partition.ReceiveEvents(ctx, 1, nil)\n'
                           ' storage, err := blob.NewClient(endpoint, credential, nil)\n'
                           ' storage.UploadBuffer(ctx, "uploads-go", object, data, nil)\n'
                           ' storage.DownloadBuffer(ctx, "uploads-go", object, data, nil)\n'
                           ' secret, err := secrets.NewClient(vault, credential, nil)\n'
                           ' secret.GetSecret(ctx, "token-go", "", nil)\n'
                           '}\n'
                           '\n'
                           'func variants() {\n'
                           ' client, _ := bus.NewClientFromConnectionString(connection, nil)\n'
                           ' sender, _ := client.NewSender("variant-go", nil)\n'
                           ' sender.SendMessageBatch(ctx, batch, nil)\n'
                           ' producer, _ := hub.NewProducerClientFromConnectionString(connection, '
                           '"variant-go", nil)\n'
                           ' producer.SendEventDataBatch(ctx, batch, nil)\n'
                           ' consumer, _ := hub.NewConsumerClientFromConnectionString(connection, '
                           '"variant-go", group, nil)\n'
                           ' partition, _ := consumer.NewPartitionClient(unknownPartition, nil)\n'
                           ' partition.ReceiveEvents(ctx, 1, nil)\n'
                           ' first, _ := blob.NewClientWithNoCredential(endpoint, nil)\n'
                           ' first.UploadFile(ctx, "variant-go", unknownObject, file, nil)\n'
                           ' first.DownloadFile(ctx, "variant-go", unknownObject, file, nil)\n'
                           ' second, _ := blob.NewClientWithSharedKeyCredential(endpoint, '
                           'credential, nil)\n'
                           ' second.UploadStream(ctx, "variant-go", unknownObject, stream, nil)\n'
                           ' second.DownloadStream(ctx, "variant-go", unknownObject, nil)\n'
                           ' third, _ := blob.NewClientFromConnectionString(connection, nil)\n'
                           ' third.UploadBuffer(ctx, "factory-go", unknownObject, data, nil)\n'
                           ' third.DownloadBuffer(ctx, "factory-go", unknownObject, data, nil)\n'
                           '}\n',
              'worker.js': "import { ServiceBusClient as Bus } from '@azure/service-bus';\n"
                           'import { EventHubProducerClient as Producer, EventHubConsumerClient as '
                           "Consumer } from '@azure/event-hubs';\n"
                           "import { BlobServiceClient as Storage } from '@azure/storage-blob';\n"
                           "import { SecretClient as Secrets } from '@azure/keyvault-secrets';\n"
                           'const bus = new Bus(connection);\n'
                           "const sender = bus.createSender('orders-javascript');\n"
                           "const receiver = bus.createReceiver('orders-javascript');\n"
                           "const subscription = bus.createReceiver('orders-javascript', "
                           "'audit');\n"
                           "const producer = new Producer(connection, 'events-javascript');\n"
                           "const consumer = new Consumer('group', connection, "
                           "'events-javascript');\n"
                           'const storage = new Storage(endpoint, credential);\n'
                           "const container = storage.getContainerClient('uploads-javascript');\n"
                           'const blob = container.getBlockBlobClient(unknownObject);\n'
                           'const secrets = new Secrets(vault, credential);\n'
                           'export function send() { sender.sendMessages(message); }\n'
                           'export function receive() { receiver.receiveMessages(1); }\n'
                           'export function subscribe() { subscription.subscribe({processMessage: '
                           'handler}); }\n'
                           'export function sendEvents() { producer.sendBatch(batch); }\n'
                           'export function receiveEvents() { consumer.subscribe({processEvents: '
                           'handler}); }\n'
                           'export function write() { blob.upload(data, count); }\n'
                           'export function read() { '
                           'container.getBlobClient(unknownObject).download(); }\n'
                           "export function secret() { secrets.getSecret('token-javascript'); }\n"
                           'export function hidden(Bus, Producer, Storage, Secrets) {\n'
                           ' new '
                           "Bus(connection).createSender('hidden-bus').sendMessages(message);\n"
                           " new Producer(connection, 'hidden-hub').sendBatch(batch);\n"
                           ' new Storage(endpoint, '
                           "credential).getContainerClient('hidden-container').getBlockBlobClient('object').upload(data, "
                           'count);\n'
                           " new Secrets(vault, credential).getSecret('hidden-secret');\n"
                           '}\n'
                           'function handler(message) {}\n'
                           '\n'
                           'export function writeData() { blob.uploadData(data); }\n'
                           'export function writeFile() { blob.uploadFile(file); }\n'
                           'export function writeStream() { blob.uploadStream(stream); }\n'
                           'export function readBuffer() { blob.downloadToBuffer(); }\n'
                           'export function readFile() { blob.downloadToFile(file); }\n'
                           'export function staticStorage() { '
                           "Storage.fromConnectionString(connection).getContainerClient('factory-storage').getBlockBlobClient(unknownObject).uploadData(data); "
                           '}\n'
                           'export function structuredQueue() { '
                           "bus.createSender('/subscriptions/s/resourceGroups/g/providers/Microsoft.ServiceBus/namespaces/n/queues/structured').sendMessages(message); "
                           '}\n'
                           'export function structuredSub() { '
                           "bus.createReceiver('/subscriptions/s/resourceGroups/g/providers/Microsoft.ServiceBus/namespaces/n/topics/structured', "
                           "'/subscriptions/s/resourceGroups/g/providers/Microsoft.ServiceBus/namespaces/n/topics/structured/subscriptions/audit').receiveMessages(1); "
                           '}\n'
                           'export function structuredBlob() { '
                           "storage.getContainerClient('https://account.blob.core.windows.net/structured/object?sv=version').getBlobClient(unknownObject).download(); "
                           '}\n'
                           'export function structuredSecret() { '
                           "secrets.getSecret('https://keys.vault.azure.net/secrets/structured/version'); "
                           '}\n'
                           'export function unknownHub() { new '
                           'Producer(connection).sendBatch(batch); }\n'
                           'export function wrongService() { '
                           "bus.createSender('/subscriptions/s/resourceGroups/g/providers/Microsoft.EventHub/namespaces/n/eventhubs/wrong').sendMessages(message); "
                           '}\n'
                           '\n'
                           'export function repeatReads() { blob.download(); blob.download(); '
                           "secrets.getSecret('repeat'); secrets.getSecret('repeat'); }\n",
              'Worker.cs': 'using Azure.Messaging.ServiceBus;\n'
                           'using Azure.Messaging.EventHubs.Producer;\n'
                           'using Azure.Messaging.EventHubs.Consumer;\n'
                           'using Azure.Storage.Blobs;\n'
                           'using Azure.Security.KeyVault.Secrets;\n'
                           'class Worker {\n'
                           ' async Task Send() { var bus = new ServiceBusClient(connection); var '
                           'sender = bus.CreateSender("orders-csharp"); await '
                           'sender.SendMessageAsync(message); }\n'
                           ' async Task Receive() { var bus = new ServiceBusClient(connection); '
                           'var receiver = bus.CreateReceiver("orders-csharp"); await '
                           'receiver.ReceiveMessageAsync(); }\n'
                           ' async Task Subscribe() { var bus = new ServiceBusClient(connection); '
                           'var receiver = bus.CreateReceiver("orders-csharp", "audit"); await '
                           'receiver.ReceiveMessagesAsync(1); }\n'
                           ' async Task SendEvents() { var producer = new '
                           'EventHubProducerClient(connection, "events-csharp"); await '
                           'producer.SendAsync(batch); }\n'
                           ' async Task ReceiveEvents() { var consumer = new '
                           'EventHubConsumerClient("group", connection, "events-csharp"); '
                           'consumer.ReadEventsAsync(); }\n'
                           ' async Task Write() { var storage = new BlobServiceClient(connection); '
                           'var container = storage.GetBlobContainerClient("uploads-csharp"); var '
                           'blob = container.GetBlobClient(unknownObject); await '
                           'blob.UploadAsync(data); }\n'
                           ' async Task Read() { var storage = new BlobServiceClient(connection); '
                           'var blob = '
                           'storage.GetBlobContainerClient("uploads-csharp").GetBlobClient(unknownObject); '
                           'await blob.DownloadToAsync(stream); }\n'
                           ' async Task Secret() { var secret = new SecretClient(vault, '
                           'credential); await secret.GetSecretAsync("token-csharp"); }\n'
                           '\n'
                           ' async Task NamedReceiver() { await new '
                           'ServiceBusClient(connection).CreateReceiver(topicName: '
                           '"variant-csharp", subscriptionName: "audit").ReceiveMessageAsync(); }\n'
                           ' async Task NamedQueue() { await new '
                           'ServiceBusClient(connection).CreateReceiver(queueName: '
                           '"variant-csharp").ReceiveMessagesAsync(1); }\n'
                           ' void WriteSync() { new '
                           'BlobServiceClient(connection).GetBlobContainerClient(blobContainerName: '
                           '"variant-csharp").GetBlobClient(unknownObject).Upload(data); }\n'
                           ' void ReadSync() { new '
                           'BlobServiceClient(connection).GetBlobContainerClient("variant-csharp").GetBlobClient(unknownObject).DownloadTo(stream); '
                           '}\n'
                           ' void ContainerWrite() { new '
                           'BlobServiceClient(connection).GetBlobContainerClient("variant-csharp").UploadBlob(unknownObject,data); '
                           '}\n'
                           ' async Task ContainerWriteAsync() { await new '
                           'BlobServiceClient(connection).GetBlobContainerClient("variant-csharp").UploadBlobAsync(unknownObject,data); '
                           '}\n'
                           ' void SecretSync() { new '
                           'SecretClient(vault,credential).GetSecret(name: "variant-csharp"); }\n'
                           ' async Task NamespaceProducer() { await new '
                           'EventHubProducerClient(namespaceName,"variant-csharp",credential).SendAsync(batch); '
                           '}\n'
                           ' async Task NamespaceConsumer() { new '
                           'EventHubConsumerClient(group,namespaceName,"variant-csharp",credential).ReadEventsAsync(); '
                           '}\n'
                           '}\n',
              'worker.py': 'from azure.servicebus import ServiceBusClient as Bus\n'
                           'from azure.eventhub import EventHubProducerClient as Producer, '
                           'EventHubConsumerClient as Consumer\n'
                           'from azure.storage.blob import BlobServiceClient as Storage\n'
                           'from azure.keyvault.secrets import SecretClient as Secrets\n'
                           'bus = Bus(fully_qualified_namespace=namespace, credential=credential)\n'
                           "sender = bus.get_queue_sender(queue_name='orders-python')\n"
                           "receiver = bus.get_queue_receiver(queue_name='orders-python')\n"
                           'subscription = '
                           "bus.get_subscription_receiver(topic_name='orders-python', "
                           "subscription_name='audit')\n"
                           'producer = Producer(fully_qualified_namespace=namespace, '
                           "eventhub_name='events-python', credential=credential)\n"
                           'consumer = Consumer(fully_qualified_namespace=namespace, '
                           "eventhub_name='events-python', consumer_group='group', "
                           'credential=credential)\n'
                           'storage = Storage(account_url=endpoint, credential=credential)\n'
                           "blob = storage.get_blob_client(container='uploads-python', "
                           'blob=unknown_object)\n'
                           'secrets = Secrets(vault_url=vault, credential=credential)\n'
                           'def send():\n'
                           '    sender.send_messages(message)\n'
                           'def receive():\n'
                           '    receiver.receive_messages(max_message_count=1)\n'
                           'def subscribe():\n'
                           '    subscription.receive_messages(max_message_count=1)\n'
                           'def send_events():\n'
                           '    producer.send_batch(batch)\n'
                           'def receive_events():\n'
                           '    consumer.receive(on_event=handler)\n'
                           'def write():\n'
                           '    blob.upload_blob(data)\n'
                           'def read():\n'
                           '    blob.download_blob()\n'
                           'def secret():\n'
                           "    secrets.get_secret('token-python')\n"
                           'def hidden(Bus, Producer, Storage, Secrets):\n'
                           '    Bus(namespace, '
                           "credential).get_queue_sender('hidden-bus').send_messages(message)\n"
                           "    Producer(namespace, 'hidden-hub', credential).send_batch(batch)\n"
                           "    Storage(endpoint, credential).get_blob_client('hidden-container', "
                           "'object').upload_blob(data)\n"
                           "    Secrets(vault, credential).get_secret('hidden-secret')\n"
                           'def handler(message):\n'
                           '    pass\n'
                           '\n'
                           'from azure.servicebus.aio import ServiceBusClient as AsyncBus\n'
                           'from azure.eventhub.aio import EventHubProducerClient as '
                           'AsyncProducer, EventHubConsumerClient as AsyncConsumer\n'
                           'from azure.storage.blob.aio import BlobServiceClient as AsyncStorage\n'
                           'from azure.keyvault.secrets.aio import SecretClient as AsyncSecrets\n'
                           'def topic_sender():\n'
                           '    '
                           "Bus.from_connection_string(connection).get_topic_sender('factory-topic-python').send_messages(message)\n"
                           'def factory_hub():\n'
                           '    Producer.from_connection_string(connection, '
                           "eventhub_name='factory-hub-python').send_batch(batch)\n"
                           'def factory_consumer():\n'
                           '    Consumer.from_connection_string(connection, '
                           "consumer_group='group', "
                           "eventhub_name='factory-hub-python').receive(handler)\n"
                           'def factory_blob():\n'
                           '    '
                           "Storage.from_connection_string(connection).get_container_client('factory-container-python').get_blob_client(unknown_object).upload_blob(data)\n"
                           'def container_read():\n'
                           '    '
                           "storage.get_container_client('factory-container-python').get_blob_client(unknown_object).download_blob()\n"
                           'def context_bus():\n'
                           '    with Bus.from_connection_string(connection) as client:\n'
                           "        with client.get_queue_sender('context-python') as queue:\n"
                           '            queue.send_messages(message)\n'
                           'async def aio_send():\n'
                           '    async with AsyncBus.from_connection_string(connection) as client:\n'
                           "        sender = client.get_queue_sender('aio-python')\n"
                           '        await sender.send_messages(message)\n'
                           'async def aio_receive():\n'
                           '    client = AsyncBus(namespace, credential)\n'
                           "    receiver = client.get_queue_receiver('aio-python')\n"
                           '    await receiver.receive_messages(1)\n'
                           'async def aio_produce():\n'
                           "    client = AsyncProducer(namespace, 'aio-hub-python', credential)\n"
                           '    await client.send_batch(batch)\n'
                           'async def aio_consume():\n'
                           "    client = AsyncConsumer(namespace, 'aio-hub-python', "
                           "consumer_group='group', credential=credential)\n"
                           '    await client.receive(handler)\n'
                           'async def aio_write():\n'
                           '    client = AsyncStorage(endpoint, credential)\n'
                           "    await client.get_blob_client('aio-container-python', "
                           'unknown_object).upload_blob(data)\n'
                           'async def aio_read():\n'
                           '    client = AsyncStorage(endpoint, credential)\n'
                           "    await client.get_blob_client('aio-container-python', "
                           'unknown_object).download_blob()\n'
                           'async def aio_secret():\n'
                           '    await AsyncSecrets(vault, '
                           "credential).get_secret('aio-secret-python')\n",
              'Worker.java': 'import com.azure.messaging.servicebus.ServiceBusClientBuilder;\n'
                             'import com.azure.messaging.eventhubs.EventHubClientBuilder;\n'
                             'import com.azure.storage.blob.BlobServiceClientBuilder;\n'
                             'import com.azure.security.keyvault.secrets.SecretClientBuilder;\n'
                             'class Worker {\n'
                             ' void send() { new '
                             'ServiceBusClientBuilder().connectionString(connection).sender().queueName("orders-java").buildClient().sendMessage(message); '
                             '}\n'
                             ' void receive() { new '
                             'ServiceBusClientBuilder().connectionString(connection).receiver().queueName("orders-java").buildClient().receiveMessages(1); '
                             '}\n'
                             ' void subscribe() { new '
                             'ServiceBusClientBuilder().connectionString(connection).receiver().topicName("orders-java").subscriptionName("audit").buildClient().receiveMessages(1); '
                             '}\n'
                             ' void sendEvents() { new '
                             'EventHubClientBuilder().connectionString(connection,"events-java").buildProducerClient().send(batch); '
                             '}\n'
                             ' void receiveEvents() { new '
                             'EventHubClientBuilder().connectionString(connection,"events-java").consumerGroup("group").buildConsumerClient().receiveFromPartition("0",1,position); '
                             '}\n'
                             ' void write() { new '
                             'BlobServiceClientBuilder().endpoint(endpoint).credential(credential).buildClient().getBlobContainerClient("uploads-java").getBlobClient(object).upload(data); '
                             '}\n'
                             ' void read() { new '
                             'BlobServiceClientBuilder().endpoint(endpoint).buildClient().getBlobContainerClient("uploads-java").getBlobClient(object).downloadContent(); '
                             '}\n'
                             ' void secret() { new '
                             'SecretClientBuilder().vaultUrl(vault).credential(credential).buildClient().getSecret("token-java"); '
                             '}\n'
                             '\n'
                             ' void sendMany() { new '
                             'ServiceBusClientBuilder().credential(namespace,credential).sender().topicName("variant-java").buildClient().sendMessages(messages); '
                             '}\n'
                             ' void alternateOrder() { new '
                             'ServiceBusClientBuilder().connectionString(connection).receiver().subscriptionName("audit").topicName("variant-java").buildClient().receiveMessages(1); '
                             '}\n'
                             ' void credentialHubSend() { new '
                             'EventHubClientBuilder().credential(namespace,"variant-java",credential).buildProducerClient().send(batch); '
                             '}\n'
                             ' void credentialHubReceive() { new '
                             'EventHubClientBuilder().credential(namespace,"variant-java",credential).consumerGroup("group").buildConsumerClient().receiveFromPartition(partition,1,position); '
                             '}\n'
                             ' void writeFile() { new '
                             'BlobServiceClientBuilder().connectionString(connection).buildClient().getBlobContainerClient("variant-java").getBlobClient(unknownObject).uploadFromFile(file); '
                             '}\n'
                             ' void readStream() { new '
                             'BlobServiceClientBuilder().connectionString(connection).buildClient().getBlobContainerClient("variant-java").getBlobClient(unknownObject).downloadStream(stream); '
                             '}\n'
                             ' void readFile() { new '
                             'BlobServiceClientBuilder().connectionString(connection).buildClient().getBlobContainerClient("variant-java").getBlobClient(unknownObject).downloadToFile(file); '
                             '}\n'
                             '}\n'},
 'typed': {'main.tf': 'resource "azurerm_servicebus_topic" "orders" { name = "orders" }\n'
                      'resource "azurerm_servicebus_subscription" "audit" {\n'
                      ' name = "audit"\n'
                      ' topic_id = azurerm_servicebus_topic.orders.id\n'
                      '}\n'
                      'resource "azurerm_servicebus_subscription_rule" "filter" {\n'
                      ' name = "important"\n'
                      ' subscription_id = azurerm_servicebus_subscription.audit.id\n'
                      ' filter_type = "SqlFilter"\n'
                      ' sql_filter = "importance = \'high\'"\n'
                      '}\n'
                      'resource "azurerm_servicebus_queue" "slash" { name = '
                      '"orders/subscriptions/audit" }\n'
                      'resource "azurerm_linux_function_app" "worker" {\n'
                      ' name = "sdk-worker"\n'
                      ' app_settings = {\n'
                      '  TOPIC = azurerm_servicebus_topic.orders.id\n'
                      '  SUB = azurerm_servicebus_subscription.audit.id\n'
                      '  QUEUE = azurerm_servicebus_queue.slash.id\n'
                      '  WRONG = '
                      '"/subscriptions/s/resourceGroups/g/providers/Microsoft.EventHub/namespaces/n/eventhubs/orders"\n'
                      ' }\n'
                      ' site_config {\n'
                      '  application_stack {\n'
                      '   docker {\n'
                      '    registry_url = "https://ghcr.io"\n'
                      '    image_name = "probe/azure-sdk-worker"\n'
                      '    image_tag = "v1"\n'
                      '   }\n'
                      '  }\n'
                      ' }\n'
                      '}\n',
           'worker.ts': "import { ServiceBusClient } from '@azure/service-bus';\n"
                        'const bus = new ServiceBusClient(connection);\n'
                        'export function receive() { bus.createReceiver(process.env.TOPIC, '
                        'process.env.SUB).receiveMessages(1); }\n'
                        'export function queue() { '
                        'bus.createReceiver(process.env.QUEUE).receiveMessages(1); }\n'
                        'export function wrongQueue() { '
                        'bus.createReceiver(process.env.SUB).receiveMessages(1); }\n'
                        'export function wrongSubscription() { '
                        'bus.createReceiver(process.env.TOPIC, '
                        'process.env.TOPIC).receiveMessages(1); }\n'
                        'export function wrongService() { '
                        'bus.createReceiver(process.env.WRONG).receiveMessages(1); }\n'
                        'export function publish() { '
                        "bus.createSender('orders').sendMessages(message); }\n",
           '.github/workflows/build.yml': 'jobs:\n'
                                          '  publish:\n'
                                          '    steps:\n'
                                          '      - uses: docker/build-push-action@v6\n'
                                          '        with:\n'
                                          '          tags: ghcr.io/probe/azure-sdk-worker:v1\n'},
 'mutation': {'worker.ts': "import { ServiceBusClient } from '@azure/service-bus';\n"
                           'const bus = new ServiceBusClient(connection);\n'
                           'function receiver({topic, sub}) { return bus.createReceiver(topic, '
                           'sub); }\n'
                           'export function memo() {\n'
                           " const first = {topic:'memo', sub:'audit'};\n"
                           ' receiver(first).receiveMessages(1);\n'
                           " const second = {topic:'memo', sub:'audit'};\n"
                           ' second.sub = runtime;\n'
                           ' receiver(second).receiveMessages(1);\n'
                           '}\n'
                           'export function snapshot() {\n'
                           " const request = {topic:'snapshot', sub:'audit'};\n"
                           ' const held = receiver(request);\n'
                           ' request.sub = runtime;\n'
                           ' held.receiveMessages(1);\n'
                           '}\n'
                           'export function alias() {\n'
                           " const request = {topic:'alias', sub:'audit'};\n"
                           ' const other = request; other.sub = runtime;\n'
                           ' receiver(request).receiveMessages(1);\n'
                           '}\n'
                           'export function mapping() {\n'
                           " const request = {topic:'mapping', sub:'audit'};\n"
                           ' Object.assign(request, {sub:runtime});\n'
                           ' receiver(request).receiveMessages(1);\n'
                           '}\n'
                           'export function unaffected() {\n'
                           " const request = {topic:'sibling', sub:'audit'};\n"
                           " const other = {topic:'other', sub:'audit'}; other.sub = runtime;\n"
                           ' receiver(request).receiveMessages(1);\n'
                           '}\n'},
 'no-workload': {'worker.go': 'package worker\n'
                              'import "os"\n'
                              'import bus '
                              '"github.com/Azure/azure-sdk-for-go/sdk/messaging/azservicebus"\n'
                              'func receive() { client, _ := bus.NewClient(namespace, credential, '
                              'nil); receiver, _ := '
                              'client.NewReceiverForSubscription(os.Getenv("TOPIC"), '
                              'os.Getenv("SUB"), nil); receiver.ReceiveMessages(ctx, 1, nil) }\n'
                              'func queue() { client, _ := bus.NewClient(namespace, credential, '
                              'nil); receiver, _ := client.NewReceiverForQueue(os.Getenv("QUEUE"), '
                              'nil); receiver.ReceiveMessages(ctx, 1, nil) }\n'
                              'func literalTopic() { client, _ := bus.NewClient(namespace, '
                              'credential, nil); receiver, _ := '
                              'client.NewReceiverForSubscription("literal", os.Getenv("SUB"), '
                              'nil); receiver.ReceiveMessages(ctx, 1, nil) }\n'
                              'func literalSub() { client, _ := bus.NewClient(namespace, '
                              'credential, nil); receiver, _ := '
                              'client.NewReceiverForSubscription(os.Getenv("TOPIC"), "fixed", '
                              'nil); receiver.ReceiveMessages(ctx, 1, nil) }\n'
                              'func unread() { client, _ := bus.NewClient(namespace, credential, '
                              'nil); receiver, _ := '
                              'client.NewReceiverForSubscription(os.Getenv("TOPIC"), '
                              'os.Getenv("UNREAD"), nil); receiver.ReceiveMessages(ctx, 1, nil) }\n'
                              '\n'
                              'func literalQueue() { client, _ := '
                              'bus.NewClient(namespace,credential,nil); receiver, _ := '
                              'client.NewReceiverForQueue("unaffected-go",nil); '
                              'receiver.ReceiveMessages(ctx,1,nil) }\n',
                 'worker.js': "import { ServiceBusClient } from '@azure/service-bus';\n"
                              'const bus = new ServiceBusClient(connection);\n'
                              'export function receive() { bus.createReceiver(process.env.TOPIC, '
                              'process.env.SUB).receiveMessages(1); }\n'
                              'export function queue() { '
                              'bus.createReceiver(process.env.QUEUE).receiveMessages(1); }\n'
                              "export function literalTopic() { bus.createReceiver('literal', "
                              'process.env.SUB).receiveMessages(1); }\n'
                              'export function literalSub() { '
                              "bus.createReceiver(process.env.TOPIC, 'fixed').receiveMessages(1); "
                              '}\n'
                              'export function unread() { bus.createReceiver(process.env.TOPIC, '
                              'process.env.UNREAD).receiveMessages(1); }\n'
                              '\n'
                              'export function literalQueue() { '
                              "bus.createReceiver('unaffected-javascript').receiveMessages(1); }\n"
                              'function constructedReceiver({topic,sub}) { return '
                              'bus.createReceiver(topic,sub); }\n'
                              'export function mutablePair() { const '
                              'names={topic:process.env.TOPIC,sub:process.env.SUB}; '
                              'names.sub=runtime; constructedReceiver(names).receiveMessages(1); '
                              '}\n',
                 'Worker.cs': 'using System;\n'
                              'using Azure.Messaging.ServiceBus;\n'
                              'class EnvWorker {\n'
                              ' async Task Receive() { await new '
                              'ServiceBusClient(connection).CreateReceiver(Environment.GetEnvironmentVariable("TOPIC"), '
                              'Environment.GetEnvironmentVariable("SUB")).ReceiveMessagesAsync(1); '
                              '}\n'
                              ' async Task Queue() { await new '
                              'ServiceBusClient(connection).CreateReceiver(Environment.GetEnvironmentVariable("QUEUE")).ReceiveMessagesAsync(1); '
                              '}\n'
                              ' async Task LiteralTopic() { await new '
                              'ServiceBusClient(connection).CreateReceiver("literal", '
                              'Environment.GetEnvironmentVariable("SUB")).ReceiveMessagesAsync(1); '
                              '}\n'
                              ' async Task LiteralSub() { await new '
                              'ServiceBusClient(connection).CreateReceiver(Environment.GetEnvironmentVariable("TOPIC"), '
                              '"fixed").ReceiveMessagesAsync(1); }\n'
                              ' async Task Unread() { await new '
                              'ServiceBusClient(connection).CreateReceiver(Environment.GetEnvironmentVariable("TOPIC"), '
                              'Environment.GetEnvironmentVariable("UNREAD")).ReceiveMessagesAsync(1); '
                              '}\n'
                              ' async Task LiteralQueue() { await new '
                              'ServiceBusClient(connection).CreateReceiver("unaffected-csharp").ReceiveMessageAsync(); '
                              '}\n'
                              '}\n',
                 'worker.py': 'import os\n'
                              'from azure.servicebus import ServiceBusClient\n'
                              'bus = ServiceBusClient(namespace, credential)\n'
                              'def receive():\n'
                              "    bus.get_subscription_receiver(os.getenv('TOPIC'), "
                              "os.getenv('SUB')).receive_messages(1)\n"
                              'def queue():\n'
                              "    bus.get_queue_receiver(os.getenv('QUEUE')).receive_messages(1)\n"
                              'def literal_topic():\n'
                              "    bus.get_subscription_receiver('literal', "
                              "os.getenv('SUB')).receive_messages(1)\n"
                              'def literal_sub():\n'
                              "    bus.get_subscription_receiver(os.getenv('TOPIC'), "
                              "'fixed').receive_messages(1)\n"
                              'def unread():\n'
                              "    bus.get_subscription_receiver(os.getenv('TOPIC'), "
                              "os.getenv('UNREAD')).receive_messages(1)\n"
                              '\n'
                              'def literal_queue():\n'
                              '    '
                              "bus.get_queue_receiver('unaffected-python').receive_messages(1)\n",
                 'Worker.java': 'import com.azure.messaging.servicebus.ServiceBusClientBuilder;\n'
                                'class EnvWorker {\n'
                                ' void receive() { new '
                                'ServiceBusClientBuilder().connectionString(connection).receiver().topicName(System.getenv("TOPIC")).subscriptionName(System.getenv("SUB")).buildClient().receiveMessages(1); '
                                '}\n'
                                ' void queue() { new '
                                'ServiceBusClientBuilder().connectionString(connection).receiver().queueName(System.getenv("QUEUE")).buildClient().receiveMessages(1); '
                                '}\n'
                                ' void literalTopic() { new '
                                'ServiceBusClientBuilder().connectionString(connection).receiver().topicName("literal").subscriptionName(System.getenv("SUB")).buildClient().receiveMessages(1); '
                                '}\n'
                                ' void literalSub() { new '
                                'ServiceBusClientBuilder().connectionString(connection).receiver().topicName(System.getenv("TOPIC")).subscriptionName("fixed").buildClient().receiveMessages(1); '
                                '}\n'
                                ' void unread() { new '
                                'ServiceBusClientBuilder().connectionString(connection).receiver().topicName(System.getenv("TOPIC")).subscriptionName(System.getenv("UNREAD")).buildClient().receiveMessages(1); '
                                '}\n'
                                ' void literalQueue() { new '
                                'ServiceBusClientBuilder().connectionString(connection).receiver().queueName("unaffected-java").buildClient().receiveMessages(1); '
                                '}\n'
                                '}\n',
                 'worker.ts': "import { ServiceBusClient } from '@azure/service-bus';\n"
                              'const bus = new '
                              'ServiceBusClient("Endpoint=sb://sdk-canary.servicebus.windows.net/;SharedAccessKeyName=owner;SharedAccessKey=AZURE_SDK_CREDENTIAL_CANARY_670");\n'
                              'export function receive() { bus.createReceiver(process.env.TOPIC, '
                              'process.env.SUB).receiveMessages(1); }\n'
                              'export function queue() { '
                              'bus.createReceiver(process.env.QUEUE).receiveMessages(1); }\n'
                              'export function invalidName() { '
                              "bus.createReceiver('Endpoint=sb://sdk-canary.servicebus.windows.net/;SharedAccessKeyName=owner;SharedAccessKey=AZURE_SDK_CREDENTIAL_CANARY_670').receiveMessages(1); "
                              '}\n'
                              '\n'
                              'export function literalQueue() { '
                              "bus.createReceiver('unaffected-typescript').receiveMessages(1); "
                              '}\n'},
 'values': {'pods.yaml': 'apiVersion: v1\n'
                         'kind: Pod\n'
                         'metadata: {name: sdk-values}\n'
                         'spec:\n'
                         '  containers:\n'
                         '    - name: plain\n'
                         '      image: ghcr.io/probe/azure-sdk-worker:v1\n'
                         '      env:\n'
                         '        - {name: HUB, value: events-env}\n'
                         '        - {name: CONTAINER, value: uploads-env}\n'
                         '        - name: PRIVATE_SECRET\n'
                         '          valueFrom: {secretKeyRef: {name: private-selector, key: '
                         'selector}}\n'
                         '        - {name: SECRET, value: token-env}\n'
                         '        - {name: WRONG, value: '
                         '/subscriptions/s/resourceGroups/g/providers/Microsoft.ServiceBus/namespaces/n/queues/wrong}\n'
                         '    - name: structured\n'
                         '      image: ghcr.io/probe/azure-sdk-worker:v1\n'
                         '      env:\n'
                         '        - {name: HUB, value: '
                         '/subscriptions/s/resourceGroups/g/providers/Microsoft.EventHub/namespaces/n/eventhubs/events-env}\n'
                         '        - {name: CONTAINER, value: '
                         "'https://account.blob.core.windows.net/uploads-env/object'}\n"
                         '        - name: PRIVATE_SECRET\n'
                         '          valueFrom: {secretKeyRef: {name: private-selector, key: '
                         'selector}}\n'
                         '        - {name: SECRET, value: '
                         "'https://keys.vault.azure.net/secrets/token-env/version'}\n"
                         '        - {name: WRONG, value: '
                         '/subscriptions/s/resourceGroups/g/providers/Microsoft.ServiceBus/namespaces/n/queues/wrong}\n'
                         '---\n'
                         'apiVersion: v1\n'
                         'kind: Secret\n'
                         'metadata: {name: private-selector}\n'
                         'stringData: {selector: private-selector-canary-670}\n',
            'worker.ts': 'import { EventHubProducerClient, EventHubConsumerClient } from '
                         "'@azure/event-hubs';\n"
                         "import { BlobServiceClient } from '@azure/storage-blob';\n"
                         "import { SecretClient } from '@azure/keyvault-secrets';\n"
                         'const storage = new BlobServiceClient(endpoint, credential);\n'
                         'const secrets = new SecretClient(vault, credential);\n'
                         'export function send() { new '
                         'EventHubProducerClient(connection,process.env.HUB).sendBatch(batch); }\n'
                         'export function receive() { new '
                         'EventHubConsumerClient(group,connection,process.env.HUB).subscribe(handler); '
                         '}\n'
                         'export function write() { '
                         'storage.getContainerClient(process.env.CONTAINER).getBlockBlobClient(unknownObject).uploadData(data); '
                         '}\n'
                         'export function read() { '
                         'storage.getContainerClient(process.env.CONTAINER).getBlobClient(unknownObject).download(); '
                         '}\n'
                         'export function secret() { secrets.getSecret(process.env.SECRET); }\n'
                         'export function wrongHub() { new '
                         'EventHubProducerClient(connection,process.env.WRONG).sendBatch(batch); '
                         '}\n'
                         'export function wrongBlob() { '
                         'storage.getContainerClient(process.env.WRONG).getBlobClient(unknownObject).download(); '
                         '}\n'
                         'export function wrongSecret() { secrets.getSecret(process.env.WRONG); }\n'
                         'export function missingSecret() { '
                         'secrets.getSecret(process.env.MISSING); }\n'
                         'export function privateSecret() { '
                         'secrets.getSecret(process.env.PRIVATE_SECRET); }\n',
            '.github/workflows/build.yml': 'jobs:\n'
                                           '  publish:\n'
                                           '    steps:\n'
                                           '      - uses: docker/build-push-action@v6\n'
                                           '        with:\n'
                                           '          tags: ghcr.io/probe/azure-sdk-worker:v1\n'},
 'same-handler': {'pods.yaml': 'apiVersion: v1\n'
                               'kind: Pod\n'
                               'metadata: {name: same-handler}\n'
                               'spec:\n'
                               '  containers:\n'
                               '    - name: worker\n'
                               '      image: ghcr.io/probe/azure-sdk-worker:v1\n'
                               '      env: [{name: BUS, value: orders}]\n',
                  'worker.go': 'package worker\n'
                               'import "os"\n'
                               'import bus '
                               '"github.com/Azure/azure-sdk-for-go/sdk/messaging/azservicebus"\n'
                               'func run() {\n'
                               ' client, _ := bus.NewClient(namespace,credential,nil)\n'
                               ' queue, _ := client.NewReceiverForQueue(os.Getenv("BUS"),nil)\n'
                               ' subscription, _ := '
                               'client.NewReceiverForSubscription(os.Getenv("BUS"),"audit",nil)\n'
                               ' queue.ReceiveMessages(ctx,1,nil)\n'
                               ' subscription.ReceiveMessages(ctx,1,nil)\n'
                               '}\n',
                  '.github/workflows/build.yml': 'jobs:\n'
                                                 '  publish:\n'
                                                 '    steps:\n'
                                                 '      - uses: docker/build-push-action@v6\n'
                                                 '        with:\n'
                                                 '          tags: '
                                                 'ghcr.io/probe/azure-sdk-worker:v1\n'},
 'overloads': {'worker.ts': "import { ServiceBusClient } from '@azure/service-bus';\n"
                            'const bus = new ServiceBusClient(connection);\n'
                            'export function inlineOptions() { '
                            "bus.createReceiver('typescript-inline', "
                            "{receiveMode:'peekLock'}).receiveMessages(1); }\n"
                            'export function namedOptions() { const '
                            "options={receiveMode:'peekLock'}; "
                            "bus.createReceiver('typescript-named',options).receiveMessages(1); }\n"
                            'export function undefinedOptions() { '
                            "bus.createReceiver('typescript-undefined',undefined).receiveMessages(1); "
                            '}\n'
                            'export function changedFields() { const '
                            "options={receiveMode:'peekLock'}; options.receiveMode=runtime; "
                            "bus.createReceiver('typescript-fields',options).receiveMessages(1); "
                            '}\n'
                            "export function reassigned() { let options={receiveMode:'peekLock'}; "
                            'options=opaque; '
                            "bus.createReceiver('unproven-typescript-reassigned',options).receiveMessages(1); "
                            '}\n'
                            'export function unknownOptions(options) { '
                            "bus.createReceiver('unproven-typescript-unknown',options).receiveMessages(1); "
                            '}\n'
                            'export function hiddenUndefined(undefined) { '
                            "bus.createReceiver('unproven-typescript-shadow',undefined).receiveMessages(1); "
                            '}\n'
                            "export function stringSubscription() { const subscription='audit'; "
                            "bus.createReceiver('typescript-topic',subscription,{receiveMode:'peekLock'}).receiveMessages(1); "
                            '}\n'
                            'function make(options) { return '
                            "bus.createReceiver('typescript-factory',options); }\n"
                            'export function exactFactories() { '
                            "make({receiveMode:'peekLock'}).receiveMessages(1); "
                            'make(opaque).receiveMessages(1); }\n'
                            'export function unaffected() { '
                            "bus.createReceiver('typescript-unaffected').receiveMessages(1); }\n",
               'worker.js': "import { ServiceBusClient } from '@azure/service-bus';\n"
                            'const bus = new ServiceBusClient(connection);\n'
                            'export function inlineOptions() { '
                            "bus.createReceiver('javascript-inline', "
                            "{receiveMode:'peekLock'}).receiveMessages(1); }\n"
                            'export function namedOptions() { const '
                            "options={receiveMode:'peekLock'}; "
                            "bus.createReceiver('javascript-named',options).receiveMessages(1); }\n"
                            'export function undefinedOptions() { '
                            "bus.createReceiver('javascript-undefined',undefined).receiveMessages(1); "
                            '}\n'
                            'export function changedFields() { const '
                            "options={receiveMode:'peekLock'}; options.receiveMode=runtime; "
                            "bus.createReceiver('javascript-fields',options).receiveMessages(1); "
                            '}\n'
                            "export function reassigned() { let options={receiveMode:'peekLock'}; "
                            'options=opaque; '
                            "bus.createReceiver('unproven-javascript-reassigned',options).receiveMessages(1); "
                            '}\n'
                            'export function unknownOptions(options) { '
                            "bus.createReceiver('unproven-javascript-unknown',options).receiveMessages(1); "
                            '}\n'
                            'export function hiddenUndefined(undefined) { '
                            "bus.createReceiver('unproven-javascript-shadow',undefined).receiveMessages(1); "
                            '}\n'
                            "export function stringSubscription() { const subscription='audit'; "
                            "bus.createReceiver('javascript-topic',subscription,{receiveMode:'peekLock'}).receiveMessages(1); "
                            '}\n'
                            'function make(options) { return '
                            "bus.createReceiver('javascript-factory',options); }\n"
                            'export function exactFactories() { '
                            "make({receiveMode:'peekLock'}).receiveMessages(1); "
                            'make(opaque).receiveMessages(1); }\n'
                            'export function unaffected() { '
                            "bus.createReceiver('javascript-unaffected').receiveMessages(1); }\n",
               'Worker.cs': 'using Azure.Messaging.ServiceBus;\n'
                            'class Worker {\n'
                            ' async Task InlineOptions() { var bus=new '
                            'ServiceBusClient(connection); await '
                            'bus.CreateReceiver("csharp-inline",new '
                            'ServiceBusReceiverOptions()).ReceiveMessageAsync(); }\n'
                            ' async Task NamedOptions() { var bus=new '
                            'ServiceBusClient(connection); var options=new '
                            'ServiceBusReceiverOptions(); await '
                            'bus.CreateReceiver("csharp-named",options).ReceiveMessageAsync(); }\n'
                            ' async Task NullNamedOptions() { var bus=new '
                            'ServiceBusClient(connection); await '
                            'bus.CreateReceiver("csharp-null-named",options:null).ReceiveMessageAsync(); '
                            '}\n'
                            ' async Task ChangedFields() { var bus=new '
                            'ServiceBusClient(connection); var options=new '
                            'ServiceBusReceiverOptions(); options.ReceiveMode=runtime; await '
                            'bus.CreateReceiver("csharp-fields",options).ReceiveMessageAsync(); }\n'
                            ' async Task Reassigned() { var bus=new ServiceBusClient(connection); '
                            'var options=new ServiceBusReceiverOptions(); options=opaque; await '
                            'bus.CreateReceiver("unproven-csharp-reassigned",options).ReceiveMessageAsync(); '
                            '}\n'
                            ' async Task UnknownOptions(ServiceBusReceiverOptions options) { var '
                            'bus=new ServiceBusClient(connection); await '
                            'bus.CreateReceiver("unproven-csharp-unknown",options).ReceiveMessageAsync(); '
                            '}\n'
                            ' async Task StringSubscription() { var bus=new '
                            'ServiceBusClient(connection); var subscription="audit"; await '
                            'bus.CreateReceiver("csharp-topic",subscription,new '
                            'ServiceBusReceiverOptions()).ReceiveMessageAsync(); }\n'
                            ' ServiceBusReceiver Make(ServiceBusReceiverOptions options) { return '
                            'new '
                            'ServiceBusClient(connection).CreateReceiver("csharp-factory",options); '
                            '}\n'
                            ' async Task ExactFactories() { await Make(new '
                            'ServiceBusReceiverOptions()).ReceiveMessageAsync(); await '
                            'Make(opaque).ReceiveMessageAsync(); }\n'
                            ' async Task Unaffected() { var bus=new ServiceBusClient(connection); '
                            'await bus.CreateReceiver("csharp-unaffected").ReceiveMessageAsync(); '
                            '}\n'
                            ' class Hidden {\n'
                            '  class ServiceBusReceiverOptions {}\n'
                            '  async Task HiddenOptions() { var bus=new '
                            'ServiceBusClient(connection); await '
                            'bus.CreateReceiver("unproven-csharp-hidden",new '
                            'ServiceBusReceiverOptions()).ReceiveMessageAsync(); }\n'
                            ' }\n'
                            '}\n'}}

def write(root, name, content):
    target = root / name
    target.parent.mkdir(parents=True, exist_ok=True)
    target.write_text(content)


def save(root, name, value):
    write(root, name, json.dumps(value, indent=2) + '\n')


def seed(run):
    roots, oracle = {}, {}
    for language, (file, source) in sources().items():
        root = run / 'workspace' / language
        write(root, file, source)
        git(root, 'init', '-q')
        git(root, 'remote', 'add', 'origin', 'https://' + REMOTE + language + '.git')
        git(root, 'add', '.')
        git(root, '-c', 'user.name=Prove', '-c', 'user.email=prove@example.invalid', 'commit', '-qm', 'neutral Azure SDK inputs')
        roots[language] = root
        oracle[language] = {
            'providers': [f'channel::servicebus::orders-{language}', f'channel::eventhubs::events-{language}', f'resource::azureblob::uploads-{language}'],
            'consumers': [f'channel::servicebus::orders-{language}', f'channel::servicebus::subscription:orders-{language}/audit', f'channel::eventhubs::events-{language}', f'resource::azureblob::uploads-{language}', f'resource::keyvault::token-{language}'],
        }
    save(run, 'oracle.json', oracle)
    save(run, 'inputs.sha256.json', {str(p.relative_to(run)): hashlib.sha256(p.read_bytes()).hexdigest() for root in roots.values() for p in root.rglob('*') if p.is_file() and '.git' not in p.parts})
    return roots, oracle


def capture(binary, root, run, label, *arguments):
    result = subprocess.run([binary, *arguments], cwd=root, text=True, capture_output=True, timeout=120)
    write(run, label + '.out', result.stdout)
    write(run, label + '.err', result.stderr)
    write(run, label + '.exit', str(result.returncode) + '\n')
    assert result.returncode == 0, result.stderr
    return result.stdout


def judge(data, oracle):
    for role in ('providers', 'consumers'):
        assert Counter(s['contract'] for s in data[role]) == Counter(oracle[role]), (role, data[role], oracle[role])
        assert all('identity' not in side for side in data[role]), data[role]
    assert len(data['links']) == 3, data['links']
    assert all(link['level'] == 'exact' for link in data['links']), data['links']
    assert 'pending-workload-env:' not in json.dumps(data)


def transports(binary, root, language, data, run, http):
    row = next(p for p in data['providers'] if p['contract'] == f'channel::servicebus::orders-{language}')
    symbol = row['site'].rsplit('::', 1)[0]
    queries = [
        {'tool': 'search', 'arguments': {'query': 'orders-' + language, 'kind': 'contract', 'budget': 12000}},
        {'tool': 'context', 'arguments': {'symbol': symbol, 'budget': 20000}},
        {'tool': 'trace', 'arguments': {'from': symbol, 'direction': 'up', 'budget': 20000}},
        {'tool': 'search', 'arguments': {'query': 'zz_prove_missing_azure_sdk', 'budget': 300}},
    ]
    blob_reader = next(c for c in data['consumers'] if c['contract'] == f'resource::azureblob::uploads-{language}')
    hub_reader = next(c for c in data['consumers'] if c['contract'] == f'channel::eventhubs::events-{language}')
    queries.extend([
        {'tool': 'search', 'arguments': {'query': 'uploads-' + language, 'kind': 'contract', 'budget': 12000}},
        {'tool': 'context', 'arguments': {'symbol': blob_reader['site'].rsplit('::', 1)[0], 'budget': 20000}},
        {'tool': 'trace', 'arguments': {'from': blob_reader['site'], 'direction': 'up', 'budget': 20000}},
        {'tool': 'search', 'arguments': {'query': 'events-' + language, 'kind': 'contract', 'budget': 12000}},
        {'tool': 'context', 'arguments': {'symbol': hub_reader['site'], 'budget': 20000}},
        {'tool': 'search', 'arguments': {'query': 'token-' + language, 'kind': 'contract', 'budget': 12000}},
    ])
    tools, texts, _ = mcp_answers(binary, root, queries, run / (language + '.stdio.err'))
    save(run, language + '.queries.json', queries)
    save(run, language + '.stdio.json', texts)
    assert all(texts) and 'channel::servicebus::' in texts[0], texts
    assert 'orders-' + language in texts[1], texts[1]
    assert 'ReceiverIdentity' not in texts[1], texts[1]
    assert 'resource::azureblob::uploads-' + language in texts[4] and 'resource::azureblob::uploads-' + language in texts[5], texts[4:7]
    assert 'channel::eventhubs::events-' + language in texts[7] and 'channel::eventhubs::events-' + language in texts[8], texts[7:9]
    assert 'resource::keyvault::token-' + language in texts[9], texts[9]
    if not http:
        return
    manifest = run / (language + '.workspace.toml')
    write(run, manifest.name, f'index_dir = "{language}-indexes"\n[[repository]]\nrepo = "{REMOTE}{language}"\npath = "{root}"\n')
    capture(binary, run, run, language + '.index', 'index', '--workspace', str(manifest))
    port = free_port()
    with (run / (language + '.http.out')).open('w') as out, (run / (language + '.http.err')).open('w') as err:
        server = subprocess.Popen([binary, 'mcp', '--listen', f'127.0.0.1:{port}', '--workspace', str(manifest)], cwd=run, stdout=out, stderr=err)
        try:
            for _ in range(100):
                try:
                    socket.create_connection(('127.0.0.1', port), timeout=1).close()
                    break
                except OSError:
                    time.sleep(.1)
            client = Http(f'http://127.0.0.1:{port}/{REMOTE}{language}')
            assert client.call('tools/list', {})['result']['tools'] == tools
            replies = [answer(client.call('tools/call', {'name': q['tool'], 'arguments': q['arguments']})) for q in queries]
            save(run, language + '.http.json', replies)
            assert replies == texts
        finally:
            server.terminate()
            server.wait(timeout=30)


def extra_oracle(minimum):
    sb, eh, blob, kv = 'channel::servicebus::', 'channel::eventhubs::', 'resource::azureblob::', 'resource::keyvault::'
    p, c = [], []
    for lang in ['javascript','typescript','python','java','go']:
        p += [sb + f'factory-first-{lang}', sb + f'factory-second-{lang}', sb + f'unaffected-{lang}']
    for lang in ['javascript','typescript','python','java','go','csharp']:
        c += [sb + f'subscription:topic-first-{lang}/audit', sb + f'subscription:topic-second-{lang}/ops']
    c += [sb+'unaffected-csharp']
    out={'origins': {'providers':p, 'consumers':c}}
    c=[]
    for _ in range(6):
        c += [sb+'orders/subscriptions/audit',sb+'shipping/subscriptions/ops',sb+'blocked/subscriptions/secret',sb+'subscription:orders/audit',sb+'subscription:shipping/ops']
    for _ in range(5):
        c += ['channel::servicebus',sb+'subscription:literal/audit',sb+'subscription:literal/ops',sb+'subscription:orders/fixed',sb+'subscription:shipping/fixed',sb+'subscription:blocked/fixed']
    image='image::ghcr.io/probe/azure-sdk-worker'
    literals=[sb+'unaffected-'+lang for lang in ['javascript','typescript','python','java','go','csharp']]
    out['environment']={'providers':[image],'consumers':c+[image,'channel::servicebus','channel::servicebus']+literals}
    out['no-workload']={'providers':[],'consumers':['channel::servicebus']*29+literals}
    out['typed']={'providers':[image,sb+'orders'],'consumers':[image,sb+'subscription:orders/audit',sb+'orders/subscriptions/audit']+['channel::servicebus']*3}
    p=[name for row in minimum.values() for name in row['providers']]
    c=[name for row in minimum.values() for name in row['consumers']]
    for lang in ['javascript','typescript']:
        p += [blob+'uploads-'+lang]*3+[blob+'factory-storage',sb+'structured','channel::eventhubs','channel::servicebus']
        c += [blob+'uploads-'+lang]*2+[sb+'subscription:structured/audit',blob+'structured',kv+'structured',blob+'uploads-'+lang,blob+'uploads-'+lang,kv+'repeat',kv+'repeat']
    p += [sb+'factory-topic-python',eh+'factory-hub-python',blob+'factory-container-python',sb+'context-python',sb+'aio-python',eh+'aio-hub-python',blob+'aio-container-python']
    c += [eh+'factory-hub-python',blob+'factory-container-python',sb+'aio-python',eh+'aio-hub-python',blob+'aio-container-python',kv+'aio-secret-python']
    p += [sb+'variant-java',eh+'variant-java',blob+'variant-java']
    c += [sb+'subscription:variant-java/audit',eh+'variant-java',blob+'variant-java',blob+'variant-java']
    p += [sb+'variant-go',eh+'variant-go',blob+'variant-go',blob+'variant-go',blob+'factory-go']
    c += [eh+'variant-go',blob+'variant-go',blob+'variant-go',blob+'factory-go']
    p += [blob+'variant-csharp']*3+[eh+'variant-csharp']
    c += [sb+'subscription:variant-csharp/audit',sb+'variant-csharp',blob+'variant-csharp',kv+'variant-csharp',eh+'variant-csharp']
    out['variants']={'providers':p,'consumers':c}
    out['mutation']={'providers':[],'consumers':[sb+'subscription:memo/audit',sb+'subscription:snapshot/audit',sb+'subscription:sibling/audit']+['channel::servicebus']*3}
    out['values']={'providers':[image,eh+'events-env',blob+'uploads-env','channel::eventhubs'],'consumers':[image,eh+'events-env',blob+'uploads-env',kv+'token-env','resource::azureblob','resource::keyvault','resource::keyvault','resource::keyvault']}
    out['same-handler']={'providers':[image],'consumers':[image,sb+'orders',sb+'subscription:orders/audit']}
    receivers=[]
    for language in ['javascript','typescript','csharp']:
        receivers += [sb+language+'-'+suffix for suffix in ['inline','named','fields','factory','unaffected']]
        receivers += [sb+language+('-null-named' if language=='csharp' else '-undefined'),sb+'subscription:'+language+'-topic/audit']
    out['overloads']={'providers':[],'consumers':receivers+['channel::servicebus']*12}
    return out


def seed_extra(run, minimum):
    roots={}
    for label, files in EXTRA_SOURCES.items():
        root=run/'workspace'/label
        for name, content in files.items():
            write(root,name,content)
        git(root,'init','-q')
        git(root,'remote','add','origin','https://'+REMOTE+label+'.git')
        git(root,'add','.')
        git(root,'-c','user.name=Prove','-c','user.email=prove@example.invalid','commit','-qm','neutral Azure SDK additional evidence')
        roots[label]=root
    oracle=extra_oracle(minimum)
    save(run,'extra-oracle.json',oracle)
    save(run,'extra-inputs.sha256.json',{str(p.relative_to(run)):hashlib.sha256(p.read_bytes()).hexdigest() for root in roots.values() for p in root.rglob('*') if p.is_file() and '.git' not in p.parts})
    return roots,oracle


def extra_transports(binary,root,label,data,run,http):
    candidates=[side for role in ['providers','consumers'] for side in data[role] if side['contract'].count('::')>=2 and side['contract'].startswith('channel::servicebus::')]
    if not candidates:
        candidates=[side for role in ['providers','consumers'] for side in data[role] if side['contract'].count('::')>=2 and side['contract'].startswith(('channel::','resource::'))]
    row=next(side for side in candidates if side['contract']=='channel::servicebus::subscription:orders/audit') if label=='environment' else candidates[0]
    contract=row['contract']
    site=row['site']
    symbol=site.rsplit('::',1)[0] if '#' in site.rsplit('::',1)[-1] else site
    queries=[{'tool':'search','arguments':{'query':contract,'kind':'contract','budget':20000}}, {'tool':'context','arguments':{'symbol':symbol,'budget':30000}}, {'tool':'trace','arguments':{'from':symbol,'direction':'up','budget':30000}}]
    tools,texts,_=mcp_answers(binary,root,queries,run/(label+'.stdio.err'))
    save(run,label+'.queries.json',queries)
    save(run,label+'.stdio.json',texts)
    assert contract in texts[0],texts
    if label=='same-handler':
        assert 'channel::servicebus::orders' in texts[1] and 'channel::servicebus::subscription:orders/audit' in texts[1],texts
    assert all('AZURE_SDK_CREDENTIAL_CANARY_670' not in text for text in texts),texts
    if not http:
        return
    manifest=run/(label+'.workspace.toml')
    write(run,manifest.name,f'index_dir = "{label}-indexes"\n[[repository]]\nrepo = "{REMOTE}{label}"\npath = "{root}"\n')
    capture(binary,run,run,label+'.index','index','--workspace',str(manifest))
    removed={}
    port=free_port()
    with (run/(label+'.http.out')).open('w') as out,(run/(label+'.http.err')).open('w') as err:
        server=subprocess.Popen([binary,'mcp','--listen',f'127.0.0.1:{port}','--workspace',str(manifest)],cwd=run,stdout=out,stderr=err)
        try:
            for _ in range(100):
                try:
                    socket.create_connection(('127.0.0.1',port),timeout=1).close();break
                except OSError:
                    time.sleep(.1)
            client=Http(f'http://127.0.0.1:{port}/{REMOTE}{label}')
            assert client.call('tools/list',{})['result']['tools']==tools
            replies=[answer(client.call('tools/call',{'name':q['tool'],'arguments':q['arguments']})) for q in queries]
            save(run,label+'.http.json',replies)
            assert replies==texts,(label,replies,texts)
            if label=='environment':
                for name in EXTRA_SOURCES[label]:
                    path=root/name
                    removed[path]=path.read_bytes()
                    path.unlink()
                stored=[answer(client.call('tools/call',{'name':q['tool'],'arguments':q['arguments']})) for q in queries]
                save(run,label+'.source-deleted.http.json',stored)
                # Locations/snippets intentionally disappear with source. Stored contracts and
                # execution scope remain answerable; Engine export parity is checked separately.
                assert contract in stored[0] and contract in stored[1],stored
                assert 'index behind the mirror' in stored[1],stored
                assert 'runs code' in stored[2] and 'Pod/sdk-workers' in stored[2],stored
                assert all('AZURE_SDK_CREDENTIAL_CANARY_670' not in text for text in stored)
        finally:
            server.terminate();server.wait(timeout=30)
            for path,content in removed.items():
                path.parent.mkdir(parents=True,exist_ok=True);path.write_bytes(content)


def run_extra(binary,roots,oracles,run,http):
    import sqlite3
    for label,root in roots.items():
        text=capture(binary,root,run,label+'.export','export')
        data=json.loads(text)
        for role in ['providers','consumers']:
            assert Counter(s['contract'] for s in data[role])==Counter(oracles[label][role]),(label,role,data[role],oracles[label][role])
            assert all('identity' not in side for side in data[role]),(label,role)
        assert all(token not in text for token in ['AZURE_SDK_CREDENTIAL_CANARY_670','secret-subscription-canary-670','private-selector-canary-670'])
        if label in ['environment','no-workload']:
            with sqlite3.connect(root/'.meada/index.db') as db:
                identities=[json.loads(row[0]) for row in db.execute('SELECT identity FROM contracts WHERE identity IS NOT NULL')]
            assert identities and all(token not in json.dumps(identities) for token in ['SharedAccessKey','Endpoint','AZURE_SDK_CREDENTIAL_CANARY_670'])
            save(run,label+'.private-identities.json',identities)
        if label=='origins':
            for file in ['worker.js','worker.ts','worker.py','worker.go','Worker.java','Worker.cs']:
                assert any(call['span']['path']==file and call['level'] in ['inferred','ambiguous'] and ('::unknown.make' in call['id'] or '::foreign.' in call['id']) for call in data['calls']),file
        if label=='typed':
            delivered=[link for link in data['links'] if link['contract']=='channel::servicebus::subscription:orders/audit']
            assert len(delivered)==1 and delivered[0]['level']=='inferred',data['links']
            assert 'filter' in delivered[0]['reason'] and 'workload:' in delivered[0]['reason'],delivered
        if label=='environment':
            path=root/'pods.yaml'
            original=path.read_text()
            path.write_text(original.replace('audit','watch'))
            changed=json.loads(capture(binary,root,run,label+'.changed','export'))
            for role in ['providers','consumers']:
                expected=Counter(name.replace('/audit','/watch') for name in oracles[label][role])
                assert Counter(side['contract'] for side in changed[role])==expected,(role,changed[role],expected)
            path.write_text(original)
            restored=json.loads(capture(binary,root,run,label+'.restored','export'))
            assert restored==data
        extra_transports(binary,root,label,data,run,http)
        capture(binary,root,run,label+'.status','status')


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--head-bin')
    parser.add_argument('--run', required=True)
    parser.add_argument('--http', action='store_true')
    parser.add_argument('--seed-only', action='store_true')
    args = parser.parse_args()
    run = Path(args.run).resolve()
    run.mkdir(parents=True, exist_ok=False)
    roots, oracle = seed(run)
    extra_roots, extra_oracles = seed_extra(run, oracle)
    if args.seed_only:
        return
    assert args.head_bin, '--head-bin required except --seed-only'
    binary = str(Path(args.head_bin).resolve())
    for language, root in roots.items():
        data = json.loads(capture(binary, root, run, language + '.export', 'export'))
        judge(data, oracle[language])
        transports(binary, root, language, data, run, args.http)
        file, _ = sources()[language]
        path = root / file
        original = path.read_text()
        renamed = 'edited-' + language
        path.write_text(original.replace('orders-' + language, renamed))
        changed = json.loads(capture(binary, root, run, language + '.changed', 'export'))
        assert Counter(p['contract'] for p in changed['providers']) != Counter(p['contract'] for p in data['providers'])
        assert any(p['contract'] == 'channel::servicebus::' + renamed for p in changed['providers'])
        path.write_text(original)
        restored = json.loads(capture(binary, root, run, language + '.restored', 'export'))
        assert restored == data
    run_extra(binary, extra_roots, extra_oracles, run, args.http)
    save(run, 'checks.json', {'checks': 'passed', 'answer_key': '#670/#642; ADR0026; official Azure SDK inventory', 'independent': False, 'http': args.http, 'languages': list(roots)})
    print('Azure SDK CLI/MCP checks passed; fresh checker determines scope and verdict')


if __name__ == '__main__':
    main()
