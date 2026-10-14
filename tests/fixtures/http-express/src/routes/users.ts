import { Router } from "express";
import { removeUser } from "../handlers";

const users = Router();

// Inline handlers in one function. The last two both serve GET /api/users/{}, so the second
// one takes an ordinal.
export function register(legacy: boolean) {
  users.get("/", (_req, res) => res.json(list()));
  if (legacy) {
    users.get("/:userId", (req, res) => res.json(find(req.params.userId)));
  } else {
    users.get("/:id", (req, res) => res.json(find(req.params.id)));
  }
}

// A named handler from another file.
users.delete("/:id", removeUser);

function list() {
  return [];
}

function find(id: string) {
  return { id };
}

export default users;
