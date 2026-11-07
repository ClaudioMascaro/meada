export function workloadOrders() {
  return fetch(`${process.env.ORDERS_URL}/orders`);
}

export function publicOrders() {
  return fetch('https://shop.example.com/store/orders');
}

export function directOrders() {
  return fetch('https://orders.example.com/orders');
}
