export async function runJob() {
  return fetch(`${process.env.B_URL}/run`, { method: 'POST' });
}

export async function status() {
  return fetch(`${process.env.B_URL}/status`);
}

export async function gatewayRun() {
  return fetch(`${process.env.B_URL}/api/run`, { method: 'POST' });
}

export async function literalHost() {
  return fetch('https://b.example.com/run', { method: 'POST' });
}
