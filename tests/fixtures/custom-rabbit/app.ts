import amqp from 'amqplib';
const connection = await amqp.connect('amqp://localhost');
const channel = await connection.createChannel();
export function emitTopic() { channel.publish('events', 'order.created', Buffer.from('x')); }
export function emitFanout() { channel.publish('fan', 'anything', Buffer.from('x')); }
export function emitDirect() { channel.publish('direct', 'order.created', Buffer.from('x')); }
export function emitComposed() { channel.publish('composed-exchange', 'order.created', Buffer.from('x')); }
export function emitUnbound() { channel.publish('events', 'unbound', Buffer.from('x')); }
export async function listen() {
  await channel.consume('inbox', onInbox);
  await channel.consume('broadcast', onBroadcast);
  await channel.consume('direct-inbox', onDirect);
  await channel.consume('composed-inbox', onComposed);
  await channel.consume('events', onLookalike);
}
function onInbox(message: unknown) { return message; }
function onBroadcast(message: unknown) { return message; }
function onDirect(message: unknown) { return message; }
function onComposed(message: unknown) { return message; }
function onLookalike(message: unknown) { return message; }
