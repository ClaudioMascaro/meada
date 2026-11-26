import { svc, after, computed, before, written } from './service';
import { svc as shared } from './barrel';
declare function other(): void;

export function imported() { svc.go(); svc.list(); }
export function star() { shared.go(); }
export function importedAfter() { after.send(); }
export function importedComputed() { computed.send(); }
export function importedBefore() { before.send(); }
export function importedWritten() { written.send(); }
export function replace() { written.send = other; }
