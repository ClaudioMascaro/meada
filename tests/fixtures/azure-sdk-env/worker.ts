import { ServiceBusClient } from '@azure/service-bus';
const bus = new ServiceBusClient("Endpoint=sb://sdk-canary.servicebus.windows.net/;SharedAccessKeyName=owner;SharedAccessKey=AZURE_SDK_CREDENTIAL_CANARY_670");
export function receive() { bus.createReceiver(process.env.TOPIC, process.env.SUB).receiveMessages(1); }
export function queue() { bus.createReceiver(process.env.QUEUE).receiveMessages(1); }
export function invalidName() { bus.createReceiver('Endpoint=sb://sdk-canary.servicebus.windows.net/;SharedAccessKeyName=owner;SharedAccessKey=AZURE_SDK_CREDENTIAL_CANARY_670').receiveMessages(1); }

export function literalQueue() { bus.createReceiver('unaffected-typescript').receiveMessages(1); }
