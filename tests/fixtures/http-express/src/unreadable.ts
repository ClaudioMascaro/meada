import { Router } from "express";
import { requireAuth } from "@acme/auth";

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

// A router mounted under a parameter is served there under no prefix either; one mounted after
// a handler is served under no path (`GET /guarded`).
const plugin = Router();
plugin.get("/plugin", (_req, res) => res.send("plugin"));
export function mountPlugin(prefix: string) {
  app.use(prefix, plugin);
}
const guarded = Router();
guarded.get("/guarded", (_req, res) => res.send("guarded"));
function auth(_req, _res, next) {
  next();
}
app.use(auth, guarded);

// A package's export first may be a handler or a path: Meada cannot tell, so the router is
// served under no prefix (`GET /npm-guarded` is no provider).
const npmGuarded = Router();
npmGuarded.get("/npm-guarded", (_req, res) => res.send("npm"));
app.use(requireAuth, npmGuarded);
