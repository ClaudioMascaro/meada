export class Api {
  constructor(private readonly base: string) {}
  list() { return fetch(`${this.base}/orders`); }
}
export const api = new Api('https://orders.example.com');
