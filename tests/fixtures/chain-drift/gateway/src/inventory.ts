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

// A branch that redirects answers another way: the shape is open at the root.
api.get("/inventory/:sku/image", (req: any, res: any) => {
  if (req.query.cdn) {
    return res.redirect("/cdn");
  }
  res.json({ url: "u" });
});

// Error answers in other forms: `sendStatus`, a literal status chained to `end` and `next(err)`
// are left out of the shape like any literal status outside 2xx, so `tax` is missing.
api.get("/inventory/:sku/price", (req: any, res: any, next: any) => {
  if (!req.params.sku) {
    return res.sendStatus(400);
  }
  if (req.query.gone) {
    return res.status(NOT_FOUND).end();
  }
  try {
    res.json({ amount: 1, currency: "BRL" });
  } catch (err) {
    next(err);
  }
});
