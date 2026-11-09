import { ServiceBusClient } from '@azure/service-bus';
const bus = new ServiceBusClient(connection);
export function receive() { bus.createReceiver(process.env.TOPIC, process.env.SUB).receiveMessages(1); }
export function queue() { bus.createReceiver(process.env.QUEUE).receiveMessages(1); }
export function literalTopic() { bus.createReceiver('literal', process.env.SUB).receiveMessages(1); }
export function literalSub() { bus.createReceiver(process.env.TOPIC, 'fixed').receiveMessages(1); }
export function unread() { bus.createReceiver(process.env.TOPIC, process.env.UNREAD).receiveMessages(1); }

export function literalQueue() { bus.createReceiver('unaffected-javascript').receiveMessages(1); }
function constructedReceiver({topic,sub}) { return bus.createReceiver(topic,sub); }
export function mutablePair() { const names={topic:process.env.TOPIC,sub:process.env.SUB}; names.sub=runtime; constructedReceiver(names).receiveMessages(1); }
