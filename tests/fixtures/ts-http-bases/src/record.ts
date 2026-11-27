declare const config: any;

export class Store {
  perEnv;
  constructor() {
    this.perEnv = {
      a: { url: config.getString('a.url') },
      b: { url: config.getString('a.b.url') },
    };
  }
  list(env: string) { return fetch(`${this.perEnv[env].url}/orders`); }
}
