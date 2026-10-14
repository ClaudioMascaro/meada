import { Router } from "express";
import { cancelOrder, listOrders } from "./orders";

export const api = Router();
api.get("/orders", listOrders);
api.delete("/orders/:id", cancelOrder);
