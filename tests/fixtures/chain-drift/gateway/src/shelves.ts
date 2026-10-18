import { LIMITS, SHELF } from "@chain/contracts";
import { api } from "./routes";

// The fields of every shelf come from the shared package's constant, in another repository.
api.get("/shelves/:id", (req: any, res: any) => {
  res.json({ ...SHELF, id: req.params.id });
});

// A spread of a shared constant Meada cannot read: the listing is open.
api.get("/shelves", (req: any, res: any) => {
  res.json({ ...LIMITS, count: 1 });
});
