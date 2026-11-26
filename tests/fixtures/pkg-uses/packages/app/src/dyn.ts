export async function one() {
  const { X } = await import('@scope/a');
  return X;
}

export async function two() {
  return (await import('@scope/a')).X;
}

export async function three(s: string) {
  return import(`@scope/a${s}`);
}

export async function four() {
  return import('./client').then((m) => m.client);
}

export async function six() {
  return [(await import('@scope/a')).X, (await import('@scope/b')).X];
}
