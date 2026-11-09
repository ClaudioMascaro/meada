import { Config } from '@backstage/config';

// Read through the include Meada follows.
export async function viaFollowed(config: Config) {
  const base = config.getString('followed.url');
  return fetch(`${base}/orders`);
}

// Read through an include it does not follow: the read stays a hole.
export async function viaMissing(config: Config) {
  const base = config.getString('missing.url');
  return fetch(`${base}/orders`);
}
