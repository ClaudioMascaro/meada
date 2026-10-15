const INVENTORY = process.env.INVENTORY_URL;

// Through the base URL variable.
export function stockLevel(sku: string) {
  return fetch(`${INVENTORY}/stock/${sku}`);
}

// Through the inventory's declared host.
export function reserveStock() {
  return fetch("https://inventory.example.com/stock/reserve", { method: "POST" });
}
