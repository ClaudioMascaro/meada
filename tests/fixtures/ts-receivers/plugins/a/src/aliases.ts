import Svc from './client';
import { Sender } from './client';
import * as siblings from './client';
export function byAlias() { const c = new Sender(); c.send(); }
export function byDefault() { const c = new Svc(); c.send(); }
export function byNamespace() { const c = new siblings.Client(); c.send(); }
