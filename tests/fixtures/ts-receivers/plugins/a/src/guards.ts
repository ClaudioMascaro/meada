import { Client } from './client';
import { Outside } from 'some-lib';

interface I { send(): void }
declare class Ambient { send(): void }
interface Merged { send(): void }
class Merged {}
class Partial extends Outside {}
abstract class Abstract { abstract send(): void }
declare class Overload {
  send(x: string): void;
  send(x: number): void;
}
export function overload(c: Overload) { c.send(1); }
class T { generic() {} }
class Other { generic() {} }

export function mutable() { let c = new Client(); c.send(); }
export function structural(c: I) { c.send(); }
export function union(c: Client | I) { c.send(); }
export function nullable(c: Client | null) { c.send(); }
export function anyType(c: any) { c.send(); }
export function outside(c: Outside) { c.send(); }
export function partial(c: Partial) { c.send(); }
export function abstractMethod(c: Abstract) { c.send(); }
export function generic<T>(c: T) { c.generic(); }
export function ambient(c: Ambient) { c.send(); }
export function merged(c: Merged) { c.send(); }
export function wrappedImport() { (Outside).send(); }
