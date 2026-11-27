export async function peerRun() {
  return fetch(`${process.env.B_URL}/run`, { method: 'POST' });
}
