import { DiscoveryApi } from '@backstage/core-plugin-api';

export async function viaInventory() {
  return fetch('/api/proxy/inventory/clusters');
}

export async function viaStock() {
  return fetch('/api/proxy/stock/clusters');
}

export async function viaStockEnv() {
  return fetch('/api/proxy/stockenv/clusters');
}

export async function viaRewritten() {
  return fetch('/api/proxy/rewritten/clusters');
}

export async function viaStripped() {
  return fetch('/api/proxy/stripped/clusters');
}

export async function viaIgnored() {
  return fetch('/api/proxy/ignored/clusters');
}

export async function viaPrepended() {
  return fetch('/api/proxy/prepended/clusters');
}

export async function viaShortIgnored() {
  return fetch('/api/proxy/shortignored/clusters');
}

export async function viaShortRewritten() {
  return fetch('/api/proxy/shortrewritten/clusters');
}

export async function viaShortPrepended() {
  return fetch('/api/proxy/shortprepended/clusters');
}

export async function viaIncluded() {
  return fetch('/api/proxy/included/clusters');
}

export async function viaFromFile() {
  return fetch('/api/proxy/fromfile/clusters');
}

export async function viaFromFileShort() {
  return fetch('/api/proxy/fromfileshort/clusters');
}

export async function viaLocal() {
  return fetch('/api/proxy/local/clusters');
}

export async function viaDiscovery(discovery: DiscoveryApi) {
  const proxy = await discovery.getBaseUrl('proxy');
  return fetch(`${proxy}/inventory/clusters`);
}
