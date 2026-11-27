declare const config: any;

export class Api {
  private readonly base: string;
  constructor(base: string) { this.base = base.replace(/\/$/, ''); }
  list() { return fetch(`${this.base}/orders`); }
}

export const api = new Api(config.getString('a.url'));
