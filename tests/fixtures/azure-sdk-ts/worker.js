import { ServiceBusClient as Bus } from '@azure/service-bus';
import { EventHubProducerClient as Producer, EventHubConsumerClient as Consumer } from '@azure/event-hubs';
import { BlobServiceClient as Storage } from '@azure/storage-blob';
import { SecretClient as Secrets } from '@azure/keyvault-secrets';
const bus = new Bus(connection);
const sender = bus.createSender('orders-javascript');
const receiver = bus.createReceiver('orders-javascript');
const subscription = bus.createReceiver('orders-javascript', 'audit');
const producer = new Producer(connection, 'events-javascript');
const consumer = new Consumer('group', connection, 'events-javascript');
const storage = new Storage(endpoint, credential);
const container = storage.getContainerClient('uploads-javascript');
const blob = container.getBlockBlobClient(unknownObject);
const secrets = new Secrets(vault, credential);
export function send() { sender.sendMessages(message); }
export function receive() { receiver.receiveMessages(1); }
export function subscribe() { subscription.subscribe({processMessage: handler}); }
export function sendEvents() { producer.sendBatch(batch); }
export function receiveEvents() { consumer.subscribe({processEvents: handler}); }
export function write() { blob.upload(data, count); }
export function read() { container.getBlobClient(unknownObject).download(); }
export function secret() { secrets.getSecret('token-javascript'); }
export function hidden(Bus, Producer, Storage, Secrets) {
 new Bus(connection).createSender('hidden-bus').sendMessages(message);
 new Producer(connection, 'hidden-hub').sendBatch(batch);
 new Storage(endpoint, credential).getContainerClient('hidden-container').getBlockBlobClient('object').upload(data, count);
 new Secrets(vault, credential).getSecret('hidden-secret');
}
function handler(message) {}
