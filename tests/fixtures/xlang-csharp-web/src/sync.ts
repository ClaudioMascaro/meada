import { orderStatus, reserveOrder } from "./orders";

export async function syncOrders(id: string) {
  await orderStatus(id);
  return reserveOrder();
}
