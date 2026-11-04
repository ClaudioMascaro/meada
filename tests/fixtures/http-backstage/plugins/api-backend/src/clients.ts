import { DiscoveryService } from '@backstage/backend-plugin-api';

export async function getApproval(discovery: DiscoveryService, id: string) {
  const baseUrl = await discovery.getBaseUrl('approval');
  const response = await fetch(`${baseUrl}/requests/${id}`);
  return response.json();
}

export async function getApprovalFromBrowser(id: string) {
  const response = await fetch(`/api/approval/requests/${id}`);
  return response.json();
}

export async function getOrder(discovery: DiscoveryService, id: string) {
  const response = await fetch(`${await discovery.getBaseUrl('orders')}/requests/${id}`);
  return response.json();
}

// Not Backstage's discovery service: the base stays unknown.
export async function getSetting(settings: { getBaseUrl(id: string): Promise<string> }, id: string) {
  const baseUrl = await settings.getBaseUrl('approval');
  return fetch(`${baseUrl}/requests/${id}`);
}

// Under the orders mount, but no route of the plugin: the reason names no plugin.
export async function listOrderNotes() {
  return fetch('/api/orders/notes');
}

export async function listEntities(discovery: DiscoveryService) {
  const baseUrl = await discovery.getBaseUrl('catalog');
  return fetch(`${baseUrl}/entities`);
}

// A `getBaseUrl` in another argument is not the URL's base.
export async function withHeader(discovery: DiscoveryService, client: { base: string }) {
  return fetch(`${client.base}/requests/1`, {
    headers: { from: await discovery.getBaseUrl('approval') },
  });
}
