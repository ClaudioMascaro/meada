export async function byParam(base: string) {
  return fetch(`${base}/items`);
}

export async function plain() {
  return fetch('/orders');
}

export async function viaUrl(base: string) {
  return fetch(new URL('/orders', base));
}

export class OrdersClient {
  private base(): string {
    return process.env.API_URL + '/v2';
  }

  async list() {
    return fetch(`${this.base()}/orders`);
  }
}
