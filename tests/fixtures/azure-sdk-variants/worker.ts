import { ServiceBusClient as Bus } from '@azure/service-bus';
import { EventHubProducerClient as Producer, EventHubConsumerClient as Consumer } from '@azure/event-hubs';
import { BlobServiceClient as Storage } from '@azure/storage-blob';
import { SecretClient as Secrets } from '@azure/keyvault-secrets';
const bus = new Bus(connection);
const sender = bus.createSender('orders-typescript');
const receiver = bus.createReceiver('orders-typescript');
const subscription = bus.createReceiver('orders-typescript', 'audit');
const producer = new Producer(connection, 'events-typescript');
const consumer = new Consumer('group', connection, 'events-typescript');
const storage = new Storage(endpoint, credential);
const container = storage.getContainerClient('uploads-typescript');
const blob = container.getBlockBlobClient(unknownObject);
const secrets = new Secrets(vault, credential);
export function send() { sender.sendMessages(message); }
export function receive() { receiver.receiveMessages(1); }
export function subscribe() { subscription.subscribe({processMessage: handler}); }
export function sendEvents() { producer.sendBatch(batch); }
export function receiveEvents() { consumer.subscribe({processEvents: handler}); }
export function write() { blob.upload(data, count); }
export function read() { container.getBlobClient(unknownObject).download(); }
export function secret() { secrets.getSecret('token-typescript'); }
export function hidden(Bus, Producer, Storage, Secrets) {
 new Bus(connection).createSender('hidden-bus').sendMessages(message);
 new Producer(connection, 'hidden-hub').sendBatch(batch);
 new Storage(endpoint, credential).getContainerClient('hidden-container').getBlockBlobClient('object').upload(data, count);
 new Secrets(vault, credential).getSecret('hidden-secret');
}
function handler(message) {}

export function writeData() { blob.uploadData(data); }
export function writeFile() { blob.uploadFile(file); }
export function writeStream() { blob.uploadStream(stream); }
export function readBuffer() { blob.downloadToBuffer(); }
export function readFile() { blob.downloadToFile(file); }
export function staticStorage() { Storage.fromConnectionString(connection).getContainerClient('factory-storage').getBlockBlobClient(unknownObject).uploadData(data); }
export function structuredQueue() { bus.createSender('/subscriptions/s/resourceGroups/g/providers/Microsoft.ServiceBus/namespaces/n/queues/structured').sendMessages(message); }
export function structuredSub() { bus.createReceiver('/subscriptions/s/resourceGroups/g/providers/Microsoft.ServiceBus/namespaces/n/topics/structured', '/subscriptions/s/resourceGroups/g/providers/Microsoft.ServiceBus/namespaces/n/topics/structured/subscriptions/audit').receiveMessages(1); }
export function structuredBlob() { storage.getContainerClient('https://account.blob.core.windows.net/structured/object?sv=version').getBlobClient(unknownObject).download(); }
export function structuredSecret() { secrets.getSecret('https://keys.vault.azure.net/secrets/structured/version'); }
export function unknownHub() { new Producer(connection).sendBatch(batch); }
export function wrongService() { bus.createSender('/subscriptions/s/resourceGroups/g/providers/Microsoft.EventHub/namespaces/n/eventhubs/wrong').sendMessages(message); }

export function repeatReads() { blob.download(); blob.download(); secrets.getSecret('repeat'); secrets.getSecret('repeat'); }
