import { request } from "./api/client";
import { ORDERS_PATH } from "./api/paths";

// inferred: the wrapper's base URL comes from GATEWAY_URL, a base_url_env of the gateway.
export function listOrders() {
  return request(ORDERS_PATH);
}

export function cancelOrder(id: string) {
  return request(`${ORDERS_PATH}/${id}`, "DELETE");
}

export function createOrder(id: string) {
  return request(`${ORDERS_PATH}/${id}`, "PUT");
}
