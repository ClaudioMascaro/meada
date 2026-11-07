import { ServiceBusClient as Bus, ServiceBusSender } from '@azure/service-bus';
import { EventHubProducerClient as Producer } from '@azure/event-hubs';
import { BlobServiceClient as Storage } from '@azure/storage-blob';
import { SecretClient as Secrets } from '@azure/keyvault-secrets';
const bus = new Bus(connection);
function sender(name) { return bus.createSender(name); }
function receiver({topic, sub}) { return bus.createReceiver(topic, sub); }
export function run() {
 sender('factory-first-typescript').sendMessages(message);
 sender('factory-second-typescript').sendMessages(message);
 receiver({topic:'topic-first-typescript', sub:'audit'}).receiveMessages(1);
 receiver({topic:'topic-second-typescript', sub:'ops'}).receiveMessages(1);
}
export function hidden(Bus, Producer, Storage, Secrets) {
 new Bus(connection).createSender('hidden-bus-typescript').sendMessages(message);
 new Producer(connection, 'hidden-hub-typescript').sendBatch(batch);
 new Storage(endpoint, credential).getContainerClient('hidden-container-typescript').getBlockBlobClient('object').upload(data, count);
 new Secrets(vault, credential).getSecret('hidden-secret-typescript');
}
export function typed(client: ServiceBusSender) { client.sendMessages(message); }
export function reassigned() {
 let client = bus.createSender('stale-typescript'); client = opaque;
 client.sendMessages(message);
}
function alternative(flag) { if (flag) return bus.createSender('unsafe-typescript'); return opaque; }
export function opaquePath() { alternative(flag).sendMessages(message); }
export function nameMatched() { foreign.sender('matched-typescript').sendMessages(message); }
export function unsupported() { bus.createSender('unsupported-typescript').inventedSend(message); }
export function handleOnly() { return bus.createSender('handle-only-typescript'); }
export function unaffected() { bus.createSender('unaffected-typescript').sendMessages(message); }

class Factory { makeClient() { return new Bus(connection); } }
export function guessName(unknown) { unknown.makeClient().createSender('matched-constructor').sendMessages(message); }
