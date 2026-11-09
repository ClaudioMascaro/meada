import {PubSub} from '@google-cloud/pubsub';
const bus = new PubSub();
export function publishJs() { bus.topic(process.env.TOPIC_ID).publishMessage({data}); }
export function receiveJs() { bus.subscription(process.env.SUB_ID).on('message', handler); }
