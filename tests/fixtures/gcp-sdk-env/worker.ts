import { PubSub } from '@google-cloud/pubsub';
import { Storage } from '@google-cloud/storage';
import { SecretManagerServiceClient } from '@google-cloud/secret-manager';
const bus = new PubSub();
const storage = new Storage();
const secrets = new SecretManagerServiceClient();
export function publishPlain() { bus.topic(process.env.TOPIC_ID).publishMessage({data}); }
export function receivePlain() { bus.subscription(process.env.SUB_ID).on('message', handler); }
export function publishStructured() { bus.topic(process.env.TOPIC_PATH).publishMessage({data}); }
export function receiveStructured() { bus.subscription(process.env.SUB_PATH).on('message', handler); }
export function publishTyped() { bus.topic(process.env.TYPED_TOPIC).publishMessage({data}); }
export function receiveTyped() { bus.subscription(process.env.TYPED_SUB).on('message', handler); }
export function wrongSubscription() { bus.subscription(process.env.TYPED_TOPIC).on('message', handler); }
export function wrongTopic() { bus.topic(process.env.TYPED_SUB).publishMessage({data}); }
export function unknown() { bus.subscription(process.env.NO_SCOPE).on('message', handler); }
export function readBucket() { storage.bucket(process.env.BUCKET).file('obj').download(); }
export function readSecret() { secrets.accessSecretVersion({name: process.env.SECRET}); }
function handler(message) {}
