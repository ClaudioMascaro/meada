import { Client } from './client';
import { Thing } from 'some-lib';

async function open(): Promise<Client> { return new Client(); }
function make<T>(): T { throw new Error(); }
function outside(): Thing { throw new Error(); }

export function fromStatic() { const c = Client.make(); c.send(); }
export async function fromAsync() { const c = await open(); c.send(); }
export function withoutAwait() { const c = open(); c.send(); }
export function fromGeneric() { const c = make(); c.send(); }
export function fromOutside() { const c = outside(); c.send(); }
