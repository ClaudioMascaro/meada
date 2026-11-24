export { client } from '@app/client';

export async function five() {
  return import('@app/client').then((m) => m.client);
}
