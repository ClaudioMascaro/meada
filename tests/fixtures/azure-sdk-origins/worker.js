import { ServiceBusClient as Bus, ServiceBusSender } from '@azure/service-bus';
import { EventHubProducerClient as Producer } from '@azure/event-hubs';
import { BlobServiceClient as Storage } from '@azure/storage-blob';
import { SecretClient as Secrets } from '@azure/keyvault-secrets';
const bus = new Bus(connection);
function sender(name) { return bus.createSender(name); }
function receiver({topic, sub}) { return bus.createReceiver(topic, sub); }
export function run() {
 sender('factory-first-javascript').sendMessages(message);
 sender('factory-second-javascript').sendMessages(message);
 receiver({topic:'topic-first-javascript', sub:'audit'}).receiveMessages(1);
 receiver({topic:'topic-second-javascript', sub:'ops'}).receiveMessages(1);
}
export function hidden(Bus, Producer, Storage, Secrets) {
 new Bus(connection).createSender('hidden-bus-javascript').sendMessages(message);
 new Producer(connection, 'hidden-hub-javascript').sendBatch(batch);
 new Storage(endpoint, credential).getContainerClient('hidden-container-javascript').getBlockBlobClient('object').upload(data, count);
 new Secrets(vault, credential).getSecret('hidden-secret-javascript');
}
export function typed(client) { client.sendMessages(message); }
export function reassigned() {
 let client = bus.createSender('stale-javascript'); client = opaque;
 client.sendMessages(message);
}
function alternative(flag) { if (flag) return bus.createSender('unsafe-javascript'); return opaque; }
export function opaquePath() { alternative(flag).sendMessages(message); }
export function nameMatched() { foreign.sender('matched-javascript').sendMessages(message); }
export function unsupported() { bus.createSender('unsupported-javascript').inventedSend(message); }
export function handleOnly() { return bus.createSender('handle-only-javascript'); }
export function unaffected() { bus.createSender('unaffected-javascript').sendMessages(message); }

class Factory { makeClient() { return new Bus(connection); } }
export function guessName(unknown) { unknown.makeClient().createSender('matched-constructor').sendMessages(message); }
