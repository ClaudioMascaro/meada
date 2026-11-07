import { ServiceBusClient } from '@azure/service-bus';
const bus=new ServiceBusClient(connection);
export function send() { bus.createSender('shadowed-route-js').sendMessages(message); }
export const bare = undefined => bus.createReceiver('shadowed-route-js',undefined).receiveMessages(1);
export const commented = undefined /* parameter */ => bus.createReceiver('shadowed-route-js',undefined).receiveMessages(1);
export const parenthesized = (undefined) => bus.createReceiver('shadowed-route-js',undefined).receiveMessages(1);
export const asynchronous = async undefined => bus.createReceiver('shadowed-route-js',undefined).receiveMessages(1);
export const unaffected = options => bus.createReceiver('shadowed-route-js').receiveMessages(1);
export function builtin() { bus.createReceiver('builtin-js',undefined).receiveMessages(1); }
