import { Router } from "express";
import { cancelOrder, listOrders } from "./orders";

export const api = Router();
api.get("/orders", listOrders);
api.delete("/orders/:id", cancelOrder);
api.post("/events", (_req, res) => res.status(202).end());
