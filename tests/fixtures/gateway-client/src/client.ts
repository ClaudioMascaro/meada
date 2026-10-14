// Through the orders gateway prefix: the route behind it is GET /orders.
export const throughGateway = () => fetch("/shop/orders");

// Straight to the code path, as an internal caller does.
export const direct = () => fetch("/orders");

// The billing gateway removes /v1 and adds /billing.
export const invoices = () => fetch("/billing/invoices");

// A route that does not start with the stripped part still gets the prefix.
export const status = () => fetch("/billing/status");

// The code path of an orders route is the gateway path of a billing route.
export const audit = () => fetch("/billing/audit");

// The version is part of the code path, and goes only when `strip` says so.
export const versioned = () => fetch("/v1/invoices");
export const staleVersion = () => fetch("/billing/v1/invoices");

// The host rule picks the repository; through the gateway the link is still inferred.
export const hosted = () => fetch("https://billing.example.com/billing/invoices");
