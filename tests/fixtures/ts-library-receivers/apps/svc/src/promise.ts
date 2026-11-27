import type { Promise } from 'some-lib';
import { Store } from './log';

function open(): Promise<Store> {
  return globalThis.Promise.resolve(new Store());
}

export async function fromImportedPromise() {
  const store = await open();
  store.get('key');
}
