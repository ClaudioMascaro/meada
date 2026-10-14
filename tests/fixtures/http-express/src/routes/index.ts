import { Router } from "express";
import users from "./users";

export const api = Router();
// The second mount, imported: the users router sits under `/api/users`.
api.use("/users", users);
