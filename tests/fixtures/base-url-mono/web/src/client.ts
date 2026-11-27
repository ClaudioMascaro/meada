export async function relativeRun() {
  return fetch('/run', { method: 'POST' });
}
