import {PubSub} from '@google-cloud/pubsub';
function make({name}) { return new PubSub().topic(name); }
const options={name:'snapshot-ts'};
const first=make(options);
options.name='after-change';
const second=make(options);
export function before() { first.publishMessage({data}); }
export function after() { second.publishMessage({data}); }

function makeInline(options) {options.name='after';return new PubSub().topic(options.name);}
const inline=makeInline({name:'stale-inline-ts'});
export function inlineChanged() {inline.publishMessage({data});}
function makeBefore(options) {const handle=new PubSub().topic(options.name);options.name='after';return handle;}
const earlier=makeBefore({name:'inline-before-ts'});
export function inlineBefore() {earlier.publishMessage({data});}
function makeDefault(options={name:'stale-default-ts'}) {options.name='after';return new PubSub().topic(options.name);}
const omitted=makeDefault();
export function defaultChanged() {omitted.publishMessage({data});}
