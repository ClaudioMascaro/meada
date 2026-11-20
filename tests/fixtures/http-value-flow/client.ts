export class Client {
  private methodBase() { return process.env.API_URL; }
  private async awaitedBase() { return process.env.API_URL + '/v2'; }
  ViaMethod() { return fetch(this.methodBase() + '/api/orders'); }
  async ViaAwait() { return fetch(`${await this.awaitedBase()}/orders`); }
  ViaPromise() { return fetch(this.awaitedBase() + '/orders'); }
}

class First {
  base() { return process.env.API_URL; }
}
class Second extends First {
  base() { return process.env.SECOND_URL; }
}
export function ViaDispatch(client: First) { return fetch(client.base() + '/api/orders'); }

const functionBase = (path: string) => `${process.env.API_URL}/v2${path}`;
export function ViaFunction() { return fetch(functionBase('/orders')); }
function recursive(): string { return recursive(); }
export function ViaRecursive() { return fetch(recursive() + '/orders'); }
function divergent(flag: boolean): string {
  if (flag) return process.env.API_URL!;
  return process.env.SECOND_URL!;
}
export function ViaDivergent(flag: boolean) { return fetch(divergent(flag) + '/api/orders'); }

function envGet(key: string, path: string) { return fetch(process.env[key] + path); }
export function ParamA() { return envGet('API_URL', '/api/orders'); }
export function ParamB() { return envGet('SECOND_URL', '/api/orders'); }
export function ParamUnknown(key: string) { return envGet(key, '/api/orders'); }
export function ViaNameMatch(untyped: any) { return untyped.envGet('API_URL', '/api/orders'); }

export function QueryAppend(flag: boolean) {
  let path = '/api/orders';
  if (flag) path += '?x=1';
  return fetch(process.env.API_URL + path);
}

export function QueryRewrite(flag: boolean) {
  let path = '/api/orders';
  if (flag) path = path + '?x=1';
  return fetch(process.env.API_URL + path);
}

export function QuerySuffix(flag: boolean) {
  let query = '';
  if (flag) query = '?x=1';
  return fetch(process.env.API_URL + '/api/orders' + query);
}

export function QueryConditional(flag: boolean) {
  return fetch(`${process.env.API_URL}/api/orders${flag ? '?x=1' : ''}`);
}

function mutated(path: string) {
  path += '/other';
  return fetch(path);
}

function possiblyMutated(path: string, flag: boolean) {
  if (flag) path = '/v2/orders';
  return fetch(path);
}

function intact(path: string) { return fetch(path); }
export function Reassigned() { return mutated('https://service/api/orders'); }
export function Conditional(flag: boolean) { return possiblyMutated('https://service/api/orders', flag); }
export function Intact() { return intact('https://service/api/orders'); }

function cycle(n: number, at: string, next: string) {
  next += '/other';
  fetch(at);
  if (n > 0) cycle(n - 1, next, next);
}
export function ViaCycle() { cycle(1, '/api/orders', '/v2/orders'); }

type Discovery = { getBaseUrl(id: string): Promise<string> };

class PluginClient {
  constructor(private readonly discovery: Discovery) {}
  private async pluginBase() { return this.discovery.getBaseUrl('orders'); }
  async PluginMethod() {
    const base = await this.pluginBase();
    return fetch(`${base}/items`);
  }
  private async pluginFetch(key: string, path: string) {
    const base = await this.discovery.getBaseUrl(key);
    return fetch(new URL(`${base}${path}`));
  }
  async PluginParameter() { return this.pluginFetch('orders', '/items'); }
}
