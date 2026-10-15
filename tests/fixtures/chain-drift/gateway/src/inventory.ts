import { DEFAULTS } from "./defaults";
import { api } from "./routes";

const NOT_FOUND = 404;

// Two branches: the 404 one has fields of its own, which the shape leaves out.
export function getItem(req: { params: { sku: string } }, res: any) {
  if (!req.params.sku) {
    return res.status(NOT_FOUND).json({ error: "unknown sku", hint: "try again" });
  }
  res.json({ ...DEFAULTS, sku: req.params.sku, price: { amount: 1, currency: "BRL" } });
}

api.get("/inventory/:sku", getItem);

// An inline handler: `res.send` of a spread of the request body, which Meada cannot see.
api.post("/inventory", (req: any, res: any) => {
  res.send({ ...req.body, saved: true });
});
