import { Router } from 'express';

export async function createRouter() {
  const router = Router();
  router.get('/list', (_req, res) => res.json([]));
  return router;
}
