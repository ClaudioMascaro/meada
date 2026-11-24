export async function four() {
  return import('./client').then((m) => m.client);
}
