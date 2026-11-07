import {PubSub} from '@google-cloud/pubsub';
async function make(name) { return new PubSub().topic(name); }
export async function Awaited() {const bus=await make('await-js');bus.publishMessage({data});}
export async function LateAwait() {const pending=make('late-js');const bus=await pending;bus.publishMessage({data});}
export function Unawaited() {const pending=make('trap-pending-js');pending.publishMessage({data});}
