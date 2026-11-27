export async function viaProxy() {
  return fetch('/api/proxy/b/run', { method: 'POST' });
}
