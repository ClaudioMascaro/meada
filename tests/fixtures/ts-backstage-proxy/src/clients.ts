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

export async function viaRewrite() {
  return fetch('/api/proxy/rewritten/clusters');
}

export async function holeInEndpoint(endpoint: string) {
  return fetch(`/api/proxy/${endpoint}/clusters`);
}

export async function holeAfterEndpoint(id: string) {
  return fetch(`/api/proxy/inventory${id}`);
}

export async function viaStripped() {
  return fetch('/api/proxy/stripped/clusters');
}

export async function viaSlashed() {
  return fetch('/api/proxy/slashed/clusters');
}

export async function viaSlashedBare() {
  return fetch('/api/proxy/slashed?name=a');
}

export async function viaTwoRules() {
  return fetch('/api/proxy/twice/clusters');
}
