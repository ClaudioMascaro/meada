import { ServiceBusClient } from '@azure/service-bus';
const bus=new ServiceBusClient(connection);
export function send() { bus.createSender('shadowed-route-ts').sendMessages(message); }
export const bare = undefined => bus.createReceiver('shadowed-route-ts',undefined).receiveMessages(1);
export const commented = undefined /* parameter */ => bus.createReceiver('shadowed-route-ts',undefined).receiveMessages(1);
export const parenthesized = (undefined) => bus.createReceiver('shadowed-route-ts',undefined).receiveMessages(1);
export const asynchronous = async undefined => bus.createReceiver('shadowed-route-ts',undefined).receiveMessages(1);
export const unaffected = options => bus.createReceiver('shadowed-route-ts').receiveMessages(1);
export function builtin() { bus.createReceiver('builtin-ts',undefined).receiveMessages(1); }
