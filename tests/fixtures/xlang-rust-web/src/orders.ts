const API = process.env.API_URL;

// Through the base URL variable.
export function orderStatus(id: string) {
  return fetch(`${API}/orders/${id}`);
}

// Through the api's declared host.
export function reserveOrder() {
  return fetch("https://api.example.com/orders/reserve", { method: "POST" });
}
