import { ServiceBusClient } from '@azure/service-bus';
const bus = new ServiceBusClient(connection);
export function receive() { bus.createReceiver(process.env.TOPIC, process.env.SUB).receiveMessages(1); }
export function queue() { bus.createReceiver(process.env.QUEUE).receiveMessages(1); }
export function wrongQueue() { bus.createReceiver(process.env.SUB).receiveMessages(1); }
export function wrongSubscription() { bus.createReceiver(process.env.TOPIC, process.env.TOPIC).receiveMessages(1); }
export function wrongService() { bus.createReceiver(process.env.WRONG).receiveMessages(1); }
export function publish() { bus.createSender('orders').sendMessages(message); }
