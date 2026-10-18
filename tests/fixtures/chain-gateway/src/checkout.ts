import { productOf } from "./catalog";
import { publish } from "./events";
import { ORDERS_CREATED } from "./topics";

type Request = { query: { product: string } };

// exact: produces `orders.created` through the `publish` wrapper, the topic in an imported
// constant.
export async function createOrder(req: Request, res: { status(code: number): { end(): void } }) {
  const product = await productOf(req.query.product);
  await publish(ORDERS_CREATED, JSON.stringify(product));
  res.status(201).end();
}
