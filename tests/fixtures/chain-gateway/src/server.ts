import express from "express";
import { api } from "./routes";

const app = express();
// The mount prefix: everything `api` serves sits under `/api`.
app.use("/api", api);
app.get("/api/health", (_req, res) => res.send("ok"));

app.listen(3000);
