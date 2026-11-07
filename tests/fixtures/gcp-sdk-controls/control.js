import {PubSub} from '@google-cloud/pubsub';
import {Storage} from '@google-cloud/storage';
import {SecretManagerServiceClient} from '@google-cloud/secret-manager';
function make(name) { const bus = new PubSub(); return bus.topic(name); }
const a = make('factory-js-a');
const b = make('factory-js-b');
export function first() { a.publishMessage({data}); }
export function second() { b.publishMessage({data}); }
const bus = new PubSub();
const store = new Storage();
const secrets = new SecretManagerServiceClient();
export function envSend() { bus.topic(process.env.TOPIC_JS).publishMessage({data}); }
export function envReceive() { bus.subscription(process.env.SUB_JS).on('message', handler); }
export function envBucket() { store.bucket(process.env.BUCKET_JS).file(unknown).download(); }
export function envSecret() { secrets.accessSecretVersion({name: process.env.SECRET_JS}); }
function handler(message) {}
class Factory { makeClient() { return new PubSub(); } }
export function guessed(unknown) { const client = unknown.makeClient(); client.topic('forbidden-hop').publishMessage({data}); }
function mixed(flag, opaque) { if (flag) return new PubSub(); return opaque; }
export function opaque(flag, unknown) { const client = mixed(flag, unknown); client.topic('forbidden-branch').publishMessage({data}); }
export function reassigned(unknown) { let client = new PubSub(); client = unknown; client.topic('forbidden-write').publishMessage({data}); }
export function parameter(PubSub) { const client = new PubSub(); client.topic('forbidden-parameter').publishMessage({data}); }
export function destructured({PubSub}) { const client = new PubSub(); client.topic('forbidden-destructure').publishMessage({data}); }
export function caught() { try { run(); } catch (PubSub) { const client = new PubSub(); client.topic('forbidden-catch').publishMessage({data}); } }
export function withoutNew() { const value = PubSub(); value.topic('forbidden-without-new').publishMessage({data}); }
export function inline() { new PubSub().topic('inline-js').publishMessage({data}); }
function structured({topic}) { return new PubSub().topic(topic); }
const structuredA = structured({topic: 'structured-js-a'});
const structuredB = structured({topic: 'structured-js-b'});
export function firstStructured() { structuredA.publishMessage({data}); }
export function secondStructured() { structuredB.publishMessage({data}); }
