import { api } from "./routes";

// The catalog serves `GET /api/ping` too, so a call with no host is ambiguous between the two.
api.get("/ping", (_req: any, res: any) => {
  res.json({ ok: true });
});
