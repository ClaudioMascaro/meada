import { Config } from '@backstage/config';

export async function viaGetString(config: Config) {
  const base = config.getString('api.baseUrl');
  return fetch(`${base}/orders`);
}

export async function viaOptional(config: Config) {
  const base = config.getOptionalString('api.optionalUrl');
  return fetch(`${base}/orders`);
}

export async function viaLocalFile(config: Config) {
  const base = config.getString('api.localUrl');
  return fetch(`${base}/orders`);
}

export async function literalValue(config: Config) {
  const base = config.getString('api.literalUrl');
  return fetch(`${base}/orders`);
}

export async function missingKey(config: Config) {
  const base = config.getString('api.missing');
  return fetch(`${base}/orders`);
}

export async function notAppConfig(config: Config) {
  const base = config.getString('api.spring');
  return fetch(`${base}/orders`);
}

export async function viaIncludedKey(config: Config) {
  const base = config.getString('included.keyed.baseUrl');
  return fetch(`${base}/orders`);
}

export async function viaIncludedBlock(config: Config) {
  const base = config.getString('included.whole.api.baseUrl');
  return fetch(`${base}/orders`);
}

export async function viaEnvValue(config: Config) {
  const base = config.getString('fromEnv.baseUrl');
  return fetch(`${base}/orders`);
}
