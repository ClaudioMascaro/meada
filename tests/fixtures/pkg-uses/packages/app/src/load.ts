import { client } from './client';

export function load() {
  return client.get();
}
