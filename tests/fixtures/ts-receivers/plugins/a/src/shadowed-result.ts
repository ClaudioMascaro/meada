import { Client } from './client';
import { Client as Peer } from '../../b/src/client';

type Promise<T> = Peer & { value?: T };
function open(): Promise<Client> { return new Peer(); }

export async function shadowed() {
  const c = await open();
  return c.send();
}
