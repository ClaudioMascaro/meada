export class Orders {
  constructor(private discovery: any) {}
  async get(id: string) {
    const base = await this.discovery.getBaseUrl('orders');
    return fetch(`${base}/list/${id}`);
  }
  async put(id: string) {
    const base = await this.discovery.getBaseUrl('orders');
    return fetch(`${base}/list/${id}`, { method: 'PUT' });
  }
  fail(id: string) { return this.put(id); }
}

export function readOrder(client: Orders, id: string) { return client.get(id); }
export function updateOrder(client: Orders, id: string) { return client.fail(id); }
