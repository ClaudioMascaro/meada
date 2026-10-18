import { reserveStock, stockLevel } from "./inventory";

export async function syncInventory(sku: string) {
  await stockLevel(sku);
  return reserveStock();
}
