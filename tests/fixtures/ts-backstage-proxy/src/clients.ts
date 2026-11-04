import { DiscoveryApi } from '@backstage/core-plugin-api';

export async function viaDiscovery(discovery: DiscoveryApi) {
  const proxy = await discovery.getBaseUrl('proxy');
  return fetch(`${proxy}/inventory/clusters`);
}

export async function viaBrowserPath() {
  return fetch('/api/proxy/inventory/clusters');
}

export async function viaLiteralTarget() {
  return fetch('/api/proxy/stock/clusters');
}

export async function viaTemplate(id: string) {
  return fetch(`/api/proxy/inventory/clusters?name=${id}`);
}

export async function viaUnreadTarget() {
  return fetch('/api/proxy/legacy/clusters');
}

export async function viaUnreadAgain(discovery: DiscoveryApi) {
  const proxy = await discovery.getBaseUrl('proxy');
  return fetch(`${proxy}/legacy/clusters`);
}

export async function otherEndpoint() {
  return fetch('/api/proxy/inventoryx/clusters');
}
