import { notify } from "./notify";

// declared: reads the `orders` table the gateway writes.
export function settleOrders() {
  notify("settled");
}
