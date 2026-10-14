import { cancelOrder, listOrders } from "./orders";
import { health, track } from "./status";

export async function dashboard() {
  await health();
  await track({ url: location.href });
  return listOrders();
}

export function cancel(id: string) {
  return cancelOrder(id);
}
