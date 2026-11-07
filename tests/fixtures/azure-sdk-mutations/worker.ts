import { ServiceBusClient } from '@azure/service-bus';
const bus = new ServiceBusClient(connection);
function receiver({topic, sub}) { return bus.createReceiver(topic, sub); }
export function memo() {
 const first = {topic:'memo', sub:'audit'};
 receiver(first).receiveMessages(1);
 const second = {topic:'memo', sub:'audit'};
 second.sub = runtime;
 receiver(second).receiveMessages(1);
}
export function snapshot() {
 const request = {topic:'snapshot', sub:'audit'};
 const held = receiver(request);
 request.sub = runtime;
 held.receiveMessages(1);
}
export function alias() {
 const request = {topic:'alias', sub:'audit'};
 const other = request; other.sub = runtime;
 receiver(request).receiveMessages(1);
}
export function mapping() {
 const request = {topic:'mapping', sub:'audit'};
 Object.assign(request, {sub:runtime});
 receiver(request).receiveMessages(1);
}
export function unaffected() {
 const request = {topic:'sibling', sub:'audit'};
 const other = {topic:'other', sub:'audit'}; other.sub = runtime;
 receiver(request).receiveMessages(1);
}
