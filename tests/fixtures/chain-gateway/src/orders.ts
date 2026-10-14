import { productOf } from "./catalog";

type Request = { query: { product: string } };

export async function listOrders(req: Request, res: { json(body: unknown): void }) {
  const product = await productOf(req.query.product);
  res.json([{ id: "o1", product }]);
}

export async function cancelOrder(_req: unknown, res: { status(code: number): { end(): void } }) {
  res.status(204).end();
}
