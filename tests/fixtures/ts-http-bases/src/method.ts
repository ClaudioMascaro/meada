declare const flag: boolean;

class Client {
  constructor(private discovery: any) {}
  private async base() { return this.discovery.getBaseUrl('orders'); }
  async list() { return fetch(`${await this.base()}/list`); }
}

// A method with the same name on another class cannot supply Client's URL.
class Peer { base() { return 'https://orders.example.com'; } }
