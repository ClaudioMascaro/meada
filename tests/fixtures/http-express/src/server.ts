import express from "express";
import { api } from "./routes";

const app = express();
app.use(express.json());
// The first mount: everything `api` serves sits under `/api`.
app.use("/api", api);

// An inline handler at the top level.
app.get("/health", (_req, res) => res.send("ok"));

app.listen(3000);
