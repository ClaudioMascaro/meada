export async function viaInventory() {
  return fetch('/api/proxy/inventory/clusters');
}
