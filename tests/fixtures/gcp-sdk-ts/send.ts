import { PubSub as Bus } from '@google-cloud/pubsub';
import { Storage as Store } from '@google-cloud/storage';
import { SecretManagerServiceClient as Secrets } from '@google-cloud/secret-manager';

const bus = new Bus();
const topic = bus.topic('projects/acme/topics/orders-ts');
const subscription = bus.subscription('projects/acme/subscriptions/orders-ts');
const storage = new Store();
const object = storage.bucket('uploads-ts').file(unknownObject);
const secrets = new Secrets();
export function send() { topic.publishMessage({ data }); }
export function receive() { subscription.on('message', handle); }
export function write() { object.save(data); }
export function read() { object.download(); }
export function secret() { secrets.accessSecretVersion({ name: 'projects/acme/secrets/token-ts/versions/latest' }); }
function handle(message) {}

export function hidden(Bus, Store, Secrets) {
  const bus = new Bus();
  bus.topic('hidden-topic').publishMessage({ data });
  const storage = new Store();
  storage.bucket('hidden-bucket').file('object').save(data);
  const secrets = new Secrets();
  secrets.accessSecretVersion({name: 'hidden-secret'});
}
export function unknown(client) { client.topic('typed-only').publishMessage({ data }); }
