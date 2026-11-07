import { ServiceBusClient } from '@azure/service-bus';
const bus = new ServiceBusClient(connection);
export function inlineOptions() { bus.createReceiver('javascript-inline', {receiveMode:'peekLock'}).receiveMessages(1); }
export function namedOptions() { const options={receiveMode:'peekLock'}; bus.createReceiver('javascript-named',options).receiveMessages(1); }
export function undefinedOptions() { bus.createReceiver('javascript-undefined',undefined).receiveMessages(1); }
export function changedFields() { const options={receiveMode:'peekLock'}; options.receiveMode=runtime; bus.createReceiver('javascript-fields',options).receiveMessages(1); }
export function reassigned() { let options={receiveMode:'peekLock'}; options=opaque; bus.createReceiver('unproven-javascript-reassigned',options).receiveMessages(1); }
export function unknownOptions(options) { bus.createReceiver('unproven-javascript-unknown',options).receiveMessages(1); }
export function hiddenUndefined(undefined) { bus.createReceiver('unproven-javascript-shadow',undefined).receiveMessages(1); }
export function stringSubscription() { const subscription='audit'; bus.createReceiver('javascript-topic',subscription,{receiveMode:'peekLock'}).receiveMessages(1); }
function make(options) { return bus.createReceiver('javascript-factory',options); }
export function exactFactories() { make({receiveMode:'peekLock'}).receiveMessages(1); make(opaque).receiveMessages(1); }
export function unaffected() { bus.createReceiver('javascript-unaffected').receiveMessages(1); }
