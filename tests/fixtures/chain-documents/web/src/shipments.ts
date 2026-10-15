const BASE = process.env.GATEWAY_URL;

// The gateway's OpenAPI document lists this route, and no gateway code serves it.
export function shipment(id: string) {
  return fetch(`${BASE}/api/shipments/${id}`);
}

// exact: the gateway serves this host, and a documented route matched as written is exact.
export function publicShipment(id: string) {
  return fetch(`https://gateway.example.com/api/shipments/${id}`);
}

// The public URL of the gateway's document: another host, and its base path /gw.
export function edgeShipment(id: string) {
  return fetch(`https://api.example.com/gw/api/shipments/${id}`);
}

// A server variable with a default (region = eu, version = v3).
export function euShipment(id: string) {
  return fetch(`https://eu.example.com/v3/api/shipments/${id}`);
}

// A relative server URL gives its path: the host is unresolved.
export function relativeShipment(id: string) {
  return fetch(`/relative/base/api/shipments/${id}`);
}

// A server variable without a default gives nothing: not linked.
export function unsetShipment(id: string) {
  return fetch(`https://qa.example.com/v9/api/shipments/${id}`);
}
