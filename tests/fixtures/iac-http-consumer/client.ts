export function readOrchard(treeId: string) { return fetch(`https://orchards.example.test/orchards/${treeId}`); }
export function createOrchard() { return fetch('https://orchards.example.test/orchards', { method: 'POST' }); }
export function patchInventory(sku: string) { return fetch(`https://inventory.example.test/inventory/${sku}`, { method: 'PATCH' }); }
export function readOrbit(orbitId: string) { return fetch(`https://orbits.example.test/orbits/${orbitId}`); }
export function wrongMethod(orbitId: string) { return fetch(`https://orbits.example.test/orbits/${orbitId}`, { method: 'POST' }); }
export function wrongPath() { return fetch('https://orbits.example.test/orbits'); }
export function missingHost(orbitId: string) { return fetch(`https://unrelated.example.test/orbits/${orbitId}`); }

