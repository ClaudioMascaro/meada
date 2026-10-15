import { Router } from "express";

const app = Router();

// A path part that cannot be read (`name` is a parameter): not a provider, never `/{}/items`.
export function mountAt(name: string) {
  app.get(`/${name}/items`, (_req, res) => res.send("items"));
}

// A literal `:id` stays a provider.
app.get("/orders/:id", (_req, res) => res.send("order"));

// A router mounted under an unreadable prefix is served there under no prefix, never at the
// root; a readable mount of the same router still serves it (`GET /v1/old`).
const legacy = Router();
legacy.get("/old", (_req, res) => res.send("old"));
export function mountLegacy(version: string) {
  app.use(`/${version}/legacy`, legacy);
}
app.use("/v1", legacy);
