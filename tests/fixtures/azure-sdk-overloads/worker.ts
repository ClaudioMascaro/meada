import { ServiceBusClient } from '@azure/service-bus';
const bus = new ServiceBusClient(connection);
export function inlineOptions() { bus.createReceiver('typescript-inline', {receiveMode:'peekLock'}).receiveMessages(1); }
export function namedOptions() { const options={receiveMode:'peekLock'}; bus.createReceiver('typescript-named',options).receiveMessages(1); }
export function undefinedOptions() { bus.createReceiver('typescript-undefined',undefined).receiveMessages(1); }
export function changedFields() { const options={receiveMode:'peekLock'}; options.receiveMode=runtime; bus.createReceiver('typescript-fields',options).receiveMessages(1); }
export function reassigned() { let options={receiveMode:'peekLock'}; options=opaque; bus.createReceiver('unproven-typescript-reassigned',options).receiveMessages(1); }
export function unknownOptions(options) { bus.createReceiver('unproven-typescript-unknown',options).receiveMessages(1); }
export function hiddenUndefined(undefined) { bus.createReceiver('unproven-typescript-shadow',undefined).receiveMessages(1); }
export function stringSubscription() { const subscription='audit'; bus.createReceiver('typescript-topic',subscription,{receiveMode:'peekLock'}).receiveMessages(1); }
function make(options) { return bus.createReceiver('typescript-factory',options); }
export function exactFactories() { make({receiveMode:'peekLock'}).receiveMessages(1); make(opaque).receiveMessages(1); }
export function unaffected() { bus.createReceiver('typescript-unaffected').receiveMessages(1); }
