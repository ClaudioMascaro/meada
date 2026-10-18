// A sample of every fact the TypeScript pack stores, for the SCHEMA_VERSION guard in
// src/index.rs.
import express, { Router } from "express";
import * as path from "node:path";
import { helper } from "./helper";

export { helper as reexported } from "./helper";
export * from "./shapes";

const BASE = process.env.API_URL ?? "http://localhost";
const app = express();

export class Store {
  find(id: string, { limit = 10 }: { limit?: number }) {
    return fetch(`${BASE}/items/${id}?limit=${limit}`, { method: "GET" });
  }
}

export function serve(port = 8080, ...[first]: number[]) {
  let tries = 0;
  const { host, ...rest } = { host: "h" };
  try {
    for (const item of [tries]) helper(item, host, rest, first);
  } catch (error) {
    helper(error);
  }
  app.get("/health", (_req, res) => res.send(helper()));
  return app.listen(port, () => path.join("a", "b"));
}

// Type declarations are stored as constants (spec #23): their names, not their members.
interface Row {
  id: string;
}

type Rows = Row[];

export default Router();
