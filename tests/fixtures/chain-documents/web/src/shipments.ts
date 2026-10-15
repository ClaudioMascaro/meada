const BASE = process.env.GATEWAY_URL;

// The gateway's OpenAPI document lists this route, and no gateway code serves it.
export function shipment(id: string) {
  return fetch(`${BASE}/api/shipments/${id}`);
}

// exact: the gateway serves this host, and a documented route matched as written is exact.
export function publicShipment(id: string) {
  return fetch(`https://gateway.example.com/api/shipments/${id}`);
}
