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

export async function viaIgnorePath() {
  return fetch('/api/proxy/ignored/clusters');
}

export async function viaDisputed() {
  return fetch('/api/proxy/disputed/clusters');
}

export async function viaShort() {
  return fetch('/api/proxy/short/clusters');
}

export async function viaShortEnv() {
  return fetch('/api/proxy/shortenv/clusters');
}

export async function viaShortEnvDirect() {
  return fetch('/api/proxy/shortenvdirect/clusters');
}

export async function viaShortIgnored() {
  return fetch('/api/proxy/shortignored/clusters');
}

export async function viaShortPrepend() {
  return fetch('/api/proxy/shortprepend/clusters');
}

export async function viaShortRewrite() {
  return fetch('/api/proxy/shortrewrite/clusters');
}

export async function viaShortStripped() {
  return fetch('/api/proxy/shortstripped/clusters');
}

export async function viaIncluded() {
  return fetch('/api/proxy/included/clusters');
}

export async function viaIncludedShort() {
  return fetch('/api/proxy/includedshort/clusters');
}

export async function viaOldShape() {
  return fetch('/api/proxy/oldshape/clusters');
}

export async function viaProdShape() {
  return fetch('/api/proxy/prodshape/clusters');
}

export async function viaLocalShape() {
  return fetch('/api/proxy/localshape/clusters');
}
