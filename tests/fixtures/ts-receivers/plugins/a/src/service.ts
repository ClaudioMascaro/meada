const extra = {};
declare const key: string;

export const svc = { go() {}, list: async () => {} };
export const after = { send() {}, ...extra };
export const computed = { send() {}, [key]: () => {} };
export const before = { ...extra, send() {} };
export const written = { send() {} };

export function localAfter() { after.send(); }
export function localComputed() { computed.send(); }
export function localBefore() { before.send(); }
export function localWritten() { written.send(); }
